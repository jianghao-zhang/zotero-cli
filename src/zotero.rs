use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{activity, config::Config, date_range::DateRange};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemSummary {
    pub id: i64,
    pub key: String,
    pub item_type: String,
    pub title: Option<String>,
    pub short_title: Option<String>,
    pub citation_key: Option<String>,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub arxiv: Option<String>,
    pub url: Option<String>,
    pub date_added: Option<String>,
    pub date_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDetail {
    #[serde(flatten)]
    pub summary: ItemSummary,
    pub fields: BTreeMap<String, String>,
    pub collections: Vec<CollectionInfo>,
    pub tags: Vec<String>,
    pub attachments: Vec<AttachmentInfo>,
    pub note_count: usize,
    pub annotation_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub id: i64,
    pub key: String,
    pub title: Option<String>,
    pub path: Option<String>,
    pub resolved_path: Option<PathBuf>,
    pub content_type: Option<String>,
    pub link_mode: Option<i64>,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteInfo {
    pub id: i64,
    pub key: Option<String>,
    pub title: Option<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub date_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationInfo {
    pub id: i64,
    pub attachment_key: Option<String>,
    pub annotation_type: Option<String>,
    pub text: Option<String>,
    pub comment: Option<String>,
    pub color: Option<String>,
    pub page: Option<String>,
    pub position: Option<String>,
    pub date_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub parent_id: Option<i64>,
    pub item_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagInfo {
    pub name: String,
    pub item_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub item: ItemSummary,
    pub attachment_key: Option<String>,
    pub source: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedText {
    pub item: ItemSummary,
    pub text: String,
    pub sources: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkdownDocument {
    pub item: ItemSummary,
    pub markdown: String,
    pub source: String,
    pub source_path: Option<PathBuf>,
    pub fallback_used: bool,
    pub extracted_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingEntry {
    pub item: ItemSummary,
    pub collections: Vec<CollectionInfo>,
    pub tags: Vec<String>,
    pub attachments: Vec<AttachmentInfo>,
    pub annotation_count: usize,
    pub note_count: usize,
    pub timestamp: i64,
    pub timestamp_iso: Option<String>,
    pub provenance: String,
}

pub struct ZoteroDb {
    conn: Connection,
    storage_path: Option<PathBuf>,
}

impl ZoteroDb {
    pub fn open(config: &Config) -> Result<Self> {
        let db_path = config
            .zotero_db_path
            .as_ref()
            .ok_or_else(|| anyhow!("zotero_db_path is not configured"))?;
        let uri = sqlite_readonly_uri(db_path);
        let conn = Connection::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .or_else(|_| Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY))
        .with_context(|| format!("failed to open Zotero database {}", db_path.display()))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(Self {
            conn,
            storage_path: config.zotero_storage_path.clone(),
        })
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn table_exists(&self, table: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
                [table],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub(crate) fn column_exists(&self, table: &str, column: &str) -> bool {
        if !is_safe_ident(table) {
            return false;
        }
        let sql = format!("PRAGMA table_info({table})");
        let Ok(mut stmt) = self.conn.prepare(&sql) else {
            return false;
        };
        let Ok(columns) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
            return false;
        };
        let found = columns
            .filter_map(|row| row.ok())
            .any(|name| name.eq_ignore_ascii_case(column));
        found
    }

    pub fn list_items(&self, query: Option<&str>, limit: usize) -> Result<Vec<ItemSummary>> {
        let mut items = self.base_items(limit.saturating_mul(4).max(limit))?;
        if let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) {
            let needle = query.to_lowercase();
            items.retain(|item| {
                item.key.to_lowercase().contains(&needle)
                    || item
                        .citation_key
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&needle)
                    || item
                        .title
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&needle)
                    || item
                        .short_title
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&needle)
                    || item
                        .authors
                        .iter()
                        .any(|author| author.to_lowercase().contains(&needle))
                    || item
                        .year
                        .map(|year| year.to_string())
                        .unwrap_or_default()
                        .contains(&needle)
            });
        }
        items.truncate(limit);
        Ok(items)
    }

    pub fn resolve_items(&self, query: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let needle = query.to_lowercase();
        let query_path = PathBuf::from(query);
        let mut matches = Vec::new();

        for item in self.base_items(usize::MAX)? {
            let detail = self.item_detail_by_id(item.id)?;
            let mut score = 0_i64;
            let mut reasons = Vec::new();
            if item.key.eq_ignore_ascii_case(query) {
                score = score.max(100);
                reasons.push("key_exact");
            }
            if item
                .citation_key
                .as_deref()
                .map(|citation_key| citation_key.eq_ignore_ascii_case(query))
                .unwrap_or(false)
            {
                score = score.max(98);
                reasons.push("citation_key_exact");
            } else if item
                .citation_key
                .as_deref()
                .map(|citation_key| citation_key.to_lowercase().contains(&needle))
                .unwrap_or(false)
            {
                score = score.max(83);
                reasons.push("citation_key_contains");
            }
            if item
                .doi
                .as_deref()
                .map(|doi| doi.eq_ignore_ascii_case(query))
                .unwrap_or(false)
            {
                score = score.max(95);
                reasons.push("doi_exact");
            } else if item
                .doi
                .as_deref()
                .map(|doi| doi.to_lowercase().contains(&needle))
                .unwrap_or(false)
            {
                score = score.max(80);
                reasons.push("doi_contains");
            }
            if item
                .arxiv
                .as_deref()
                .map(|arxiv| arxiv.eq_ignore_ascii_case(query))
                .unwrap_or(false)
            {
                score = score.max(94);
                reasons.push("arxiv_exact");
            }
            if item
                .url
                .as_deref()
                .map(|url| url.eq_ignore_ascii_case(query))
                .unwrap_or(false)
            {
                score = score.max(90);
                reasons.push("url_exact");
            } else if item
                .url
                .as_deref()
                .map(|url| url.to_lowercase().contains(&needle))
                .unwrap_or(false)
            {
                score = score.max(75);
                reasons.push("url_contains");
            }
            if item
                .title
                .as_deref()
                .map(|title| title.to_lowercase().contains(&needle))
                .unwrap_or(false)
            {
                score = score.max(70);
                reasons.push("title_contains");
            }
            if item
                .short_title
                .as_deref()
                .map(|short_title| short_title.eq_ignore_ascii_case(query))
                .unwrap_or(false)
            {
                score = score.max(92);
                reasons.push("short_title_exact");
            } else if item
                .short_title
                .as_deref()
                .map(|short_title| short_title.to_lowercase().contains(&needle))
                .unwrap_or(false)
            {
                score = score.max(82);
                reasons.push("short_title_contains");
            }
            if item
                .authors
                .iter()
                .any(|author| author.to_lowercase().contains(&needle))
            {
                score = score.max(50);
                reasons.push("author_contains");
            }
            for attachment in &detail.attachments {
                let path_match = attachment
                    .resolved_path
                    .as_ref()
                    .map(|path| {
                        path == &query_path
                            || path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .map(|name| name.to_lowercase().contains(&needle))
                                .unwrap_or(false)
                            || path.to_string_lossy().to_lowercase().contains(&needle)
                    })
                    .unwrap_or(false);
                if path_match {
                    score = score.max(85);
                    reasons.push("attachment_path");
                    break;
                }
            }

            if score > 0 {
                matches.push(json!({
                    "score": score,
                    "reasons": reasons,
                    "item": item,
                }));
            }
        }
        matches.sort_by(|a, b| {
            b.get("score")
                .and_then(serde_json::Value::as_i64)
                .cmp(&a.get("score").and_then(serde_json::Value::as_i64))
        });
        matches.truncate(limit);
        Ok(matches)
    }

    pub fn find_papers(&self, query: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let needle = query.to_lowercase();
        let tokens = search_tokens(query);
        let mut hits = Vec::new();

        for item in self.base_items(usize::MAX)? {
            let fields = self.field_values(item.id)?;
            let collections = self.collections_for_item(item.id)?;
            let tags = self.tags_for_item(item.id)?;
            let mut score = 0_i64;
            let mut reasons = Vec::new();
            let mut matched = BTreeMap::new();

            score_text_field(
                "key",
                Some(item.key.as_str()),
                query,
                &needle,
                &tokens,
                FieldScore::new(100, 88, 20, 0),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "citation_key",
                item.citation_key.as_deref(),
                query,
                &needle,
                &tokens,
                FieldScore::new(98, 90, 20, 0),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "short_title",
                item.short_title.as_deref(),
                query,
                &needle,
                &tokens,
                FieldScore::new(94, 86, 18, 8),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_acronym_field(
                "short_title",
                item.short_title.as_deref(),
                &needle,
                93,
                86,
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "doi",
                item.doi.as_deref(),
                query,
                &needle,
                &tokens,
                FieldScore::new(95, 82, 16, 0),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "arxiv",
                item.arxiv.as_deref(),
                query,
                &needle,
                &tokens,
                FieldScore::new(94, 82, 16, 0),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "url",
                item.url.as_deref(),
                query,
                &needle,
                &tokens,
                FieldScore::new(90, 76, 8, 0),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "title",
                item.title.as_deref(),
                query,
                &needle,
                &tokens,
                FieldScore::new(96, 78, 12, 18),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_acronym_field(
                "title",
                item.title.as_deref(),
                &needle,
                91,
                84,
                &mut score,
                &mut reasons,
                &mut matched,
            );

            let authors = item.authors.join("; ");
            score_text_field(
                "authors",
                non_empty(authors.as_str()),
                query,
                &needle,
                &tokens,
                FieldScore::new(74, 58, 6, 8),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            score_text_field(
                "abstract",
                fields.get("abstractNote").map(String::as_str),
                query,
                &needle,
                &tokens,
                FieldScore::new(52, 44, 3, 8),
                &mut score,
                &mut reasons,
                &mut matched,
            );

            let collection_names = collections
                .iter()
                .map(|collection| collection.name.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            score_text_field(
                "collections",
                non_empty(collection_names.as_str()),
                query,
                &needle,
                &tokens,
                FieldScore::new(76, 62, 8, 10),
                &mut score,
                &mut reasons,
                &mut matched,
            );
            let tag_names = tags.join("; ");
            score_text_field(
                "tags",
                non_empty(tag_names.as_str()),
                query,
                &needle,
                &tokens,
                FieldScore::new(72, 60, 8, 10),
                &mut score,
                &mut reasons,
                &mut matched,
            );

            if score > 0 {
                hits.push(json!({
                    "score": score,
                    "reasons": reasons,
                    "matched": matched,
                    "item": item,
                }));
            }
        }

        hits.sort_by(|a, b| {
            b.get("score")
                .and_then(serde_json::Value::as_i64)
                .cmp(&a.get("score").and_then(serde_json::Value::as_i64))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn recent(&self, days: i64, limit: usize) -> Result<Vec<ItemSummary>> {
        let cutoff = Utc::now() - chrono::Duration::days(days.max(0));
        let mut items = self.base_items(limit.saturating_mul(4).max(limit))?;
        items.retain(|item| {
            item.date_modified
                .as_deref()
                .and_then(parse_db_datetime)
                .map(|dt| dt >= cutoff)
                .unwrap_or(false)
        });
        items.truncate(limit);
        Ok(items)
    }

    pub fn get_item(&self, key: &str) -> Result<ItemDetail> {
        let item_id = self
            .item_id_by_key(key)?
            .ok_or_else(|| anyhow!("Zotero item not found: {key}"))?;
        self.item_detail_by_id(item_id)
    }

    pub fn item_summary_by_id(&self, item_id: i64) -> Result<ItemSummary> {
        let row = self
            .conn
            .query_row(
                "SELECT i.itemID, i.key, COALESCE(it.typeName, 'unknown'), i.dateAdded, i.dateModified
                 FROM items i
                 LEFT JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
                 WHERE i.itemID = ?",
                [item_id],
                row_to_base_item,
            )
            .optional()?
            .ok_or_else(|| anyhow!("Zotero item id not found: {item_id}"))?;
        self.enrich_summary(row)
    }

    pub fn item_detail_by_id(&self, item_id: i64) -> Result<ItemDetail> {
        let summary = self.item_summary_by_id(item_id)?;
        let fields = self.field_values(item_id)?;
        let collections = self.collections_for_item(item_id)?;
        let tags = self.tags_for_item(item_id)?;
        let attachments = self.attachments_for_item(item_id)?;
        let note_count = self.notes_for_item(item_id)?.len();
        let annotation_count = self.annotations_for_item(item_id)?.len();
        Ok(ItemDetail {
            summary,
            fields,
            collections,
            tags,
            attachments,
            note_count,
            annotation_count,
        })
    }

    pub fn item_id_by_key(&self, key: &str) -> Result<Option<i64>> {
        self.conn
            .query_row("SELECT itemID FROM items WHERE key = ?", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn extract_text(&self, key: &str, max_chars: usize) -> Result<ExtractedText> {
        let detail = self.get_item(key)?;
        let mut chunks = Vec::new();
        let mut sources = Vec::new();

        if let Some(abstract_note) = detail.fields.get("abstractNote") {
            chunks.push(abstract_note.clone());
            sources.push("abstractNote".to_string());
        }

        for note in self.notes_for_item(detail.summary.id)? {
            if let Some(text) = note.text.filter(|value| !value.trim().is_empty()) {
                chunks.push(text);
                sources.push(format!(
                    "note:{}",
                    note.key.unwrap_or_else(|| note.id.to_string())
                ));
            }
        }

        for annotation in self.annotations_for_item(detail.summary.id)? {
            let mut text = String::new();
            if let Some(value) = annotation.text {
                text.push_str(&value);
            }
            if let Some(value) = annotation.comment {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&value);
            }
            if !text.trim().is_empty() {
                chunks.push(text);
                sources.push(format!(
                    "annotation:{}",
                    annotation
                        .attachment_key
                        .unwrap_or_else(|| annotation.id.to_string())
                ));
            }
        }

        for attachment in &detail.attachments {
            if let Some((text, source)) = self.read_attachment_text(attachment)? {
                chunks.push(text);
                sources.push(source);
            }
        }

        let mut text = chunks.join("\n\n");
        let truncated = text.chars().count() > max_chars;
        if truncated {
            text = text.chars().take(max_chars).collect();
        }
        Ok(ExtractedText {
            item: detail.summary,
            text,
            sources,
            truncated,
        })
    }

    pub fn markdown_for_item(
        &self,
        config: &Config,
        key: &str,
        max_chars: usize,
        prefer_lfz_full_md: bool,
    ) -> Result<MarkdownDocument> {
        let detail = self.get_item(key)?;
        if prefer_lfz_full_md && config.lfz.enabled.unwrap_or(false) {
            if let Some(path) = self.find_lfz_full_md(config, &detail)? {
                let markdown = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                return Ok(MarkdownDocument {
                    item: detail.summary,
                    markdown,
                    source: "llm_for_zotero_full_md".to_string(),
                    source_path: Some(path),
                    fallback_used: false,
                    extracted_truncated: false,
                });
            }
        }

        let extracted = self.extract_text(key, max_chars)?;
        let markdown = render_fallback_markdown(&detail, &extracted);
        Ok(MarkdownDocument {
            item: detail.summary,
            markdown,
            source: "zcli_fallback".to_string(),
            source_path: None,
            fallback_used: true,
            extracted_truncated: extracted.truncated,
        })
    }

    pub fn markdown_status(&self, config: &Config, key: &str) -> Result<serde_json::Value> {
        let detail = self.get_item(key)?;
        let mut candidates = Vec::new();
        if let Some(zotero_data_dir) = config.lfz.zotero_data_dir.as_ref() {
            let cache_root = zotero_data_dir.join("llm-for-zotero-mineru");
            for attachment in detail.attachments.iter().filter(|attachment| {
                is_pdf_attachment(attachment) || attachment.resolved_path.is_none()
            }) {
                let attachment_id = attachment.id.to_string();
                for path in [
                    cache_root.join(&attachment_id).join("full.md"),
                    cache_root.join(&attachment_id).join("_content.md"),
                    cache_root.join(format!("{attachment_id}.md")),
                ] {
                    candidates.push(json!({
                        "attachment_id": attachment.id,
                        "attachment_key": attachment.key,
                        "path": path,
                        "exists": path.is_file(),
                    }));
                }
            }
        }
        let found = candidates
            .iter()
            .find(|candidate| {
                candidate.get("exists").and_then(serde_json::Value::as_bool) == Some(true)
            })
            .cloned();
        Ok(json!({
            "ok": true,
            "item": detail.summary,
            "lfz_enabled": config.lfz.enabled.unwrap_or(false),
            "mineru_cache_root": config.lfz.zotero_data_dir.as_ref().map(|dir| dir.join("llm-for-zotero-mineru")),
            "preferred_source": found.as_ref().map(|_| "llm_for_zotero_full_md").unwrap_or("zcli_fallback"),
            "has_lfz_markdown": found.is_some(),
            "selected": found,
            "candidates": candidates,
            "fallback_available": true,
            "fallback_command": format!("zcli item markdown {} --no-lfz-full-md", detail.summary.key),
        }))
    }

    pub fn grep(&self, pattern: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let matcher = Regex::new(pattern).ok();
        let needle = pattern.to_lowercase();
        let mut hits = Vec::new();
        for item in self.base_items(usize::MAX)? {
            let extracted = self.extract_text(&item.key, 120_000)?;
            let text = extracted.text;
            let found = if let Some(re) = &matcher {
                re.find(&text).map(|m| m.start())
            } else {
                text.to_lowercase().find(&needle)
            };
            if let Some(offset) = found {
                hits.push(SearchHit {
                    item,
                    attachment_key: None,
                    source: "extract".to_string(),
                    snippet: snippet_at(&text, offset, 320),
                });
            }
            if hits.len() >= limit {
                break;
            }
        }
        Ok(hits)
    }

    pub fn context(
        &self,
        key: &str,
        pattern: &str,
        context_chars: usize,
    ) -> Result<serde_json::Value> {
        let extracted = self.extract_text(key, 240_000)?;
        let matcher = Regex::new(pattern).ok();
        let mut spans = Vec::new();
        if let Some(re) = matcher {
            for hit in re.find_iter(&extracted.text).take(20) {
                spans.push(json!({
                    "start": hit.start(),
                    "end": hit.end(),
                    "text": snippet_at(&extracted.text, hit.start(), context_chars),
                }));
            }
        } else {
            let haystack = extracted.text.to_lowercase();
            let needle = pattern.to_lowercase();
            let mut offset = 0;
            while let Some(pos) = haystack[offset..].find(&needle) {
                let start = offset + pos;
                spans.push(json!({
                    "start": start,
                    "end": start + needle.len(),
                    "text": snippet_at(&extracted.text, start, context_chars),
                }));
                if spans.len() >= 20 {
                    break;
                }
                offset = start + needle.len().max(1);
            }
        }
        Ok(json!({
            "ok": true,
            "item": extracted.item,
            "pattern": pattern,
            "matches": spans,
        }))
    }

    pub fn notes_for_item(&self, item_id: i64) -> Result<Vec<NoteInfo>> {
        if !self.table_exists("itemNotes") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT n.itemID, i.key, n.title, n.note, i.dateModified
             FROM itemNotes n
             LEFT JOIN items i ON n.itemID = i.itemID
             WHERE n.parentItemID = ? OR n.itemID = ?
             ORDER BY i.dateModified DESC",
        )?;
        let notes = stmt
            .query_map(params![item_id, item_id], |row| {
                let html: Option<String> = row.get(3)?;
                Ok(NoteInfo {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    title: row.get(2)?,
                    text: html.as_deref().map(strip_html),
                    html,
                    date_modified: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(notes)
    }

    pub fn annotations_for_item(&self, item_id: i64) -> Result<Vec<AnnotationInfo>> {
        if !self.table_exists("itemAnnotations") {
            return Ok(Vec::new());
        }
        let type_col = first_existing_column(self, "itemAnnotations", &["annotationType", "type"]);
        let text_col = first_existing_column(self, "itemAnnotations", &["annotationText", "text"]);
        let comment_col =
            first_existing_column(self, "itemAnnotations", &["annotationComment", "comment"]);
        let color_col =
            first_existing_column(self, "itemAnnotations", &["annotationColor", "color"]);
        let page_col = first_existing_column(
            self,
            "itemAnnotations",
            &["annotationPageLabel", "pageLabel"],
        );
        let position_col =
            first_existing_column(self, "itemAnnotations", &["annotationPosition", "position"]);
        let modified_col = first_existing_column(
            self,
            "itemAnnotations",
            &["dateModified", "annotationDateModified"],
        );

        let sql = format!(
            "SELECT ia.itemID,
                    att.key,
                    {type_col},
                    {text_col},
                    {comment_col},
                    {color_col},
                    {page_col},
                    {position_col},
                    {modified_col}
             FROM itemAnnotations ia
             LEFT JOIN items att ON ia.parentItemID = att.itemID
             LEFT JOIN itemAttachments iatt ON ia.parentItemID = iatt.itemID
             WHERE iatt.parentItemID = ? OR ia.parentItemID = ? OR ia.itemID = ?
             ORDER BY ia.itemID ASC"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![item_id, item_id, item_id], |row| {
                Ok(AnnotationInfo {
                    id: row.get(0)?,
                    attachment_key: row.get(1)?,
                    annotation_type: sql_value_to_string(row, 2)?,
                    text: sql_value_to_string(row, 3)?,
                    comment: sql_value_to_string(row, 4)?,
                    color: sql_value_to_string(row, 5)?,
                    page: sql_value_to_string(row, 6)?,
                    position: sql_value_to_string(row, 7)?,
                    date_modified: sql_value_to_string(row, 8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn attachments_for_item(&self, item_id: i64) -> Result<Vec<AttachmentInfo>> {
        if !self.table_exists("itemAttachments") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT i.itemID, i.key, ia.path, ia.contentType, ia.linkMode
             FROM itemAttachments ia
             JOIN items i ON ia.itemID = i.itemID
             WHERE ia.parentItemID = ? OR ia.itemID = ?
             ORDER BY i.itemID ASC",
        )?;
        let mut rows = stmt
            .query_map(params![item_id, item_id], |row| {
                let id: i64 = row.get(0)?;
                let key: String = row.get(1)?;
                let path: Option<String> = row.get(2)?;
                let resolved_path = self.resolve_attachment_path(&key, path.as_deref());
                let exists = resolved_path.as_deref().map(Path::exists).unwrap_or(false);
                Ok(AttachmentInfo {
                    id,
                    key,
                    title: None,
                    path,
                    resolved_path,
                    content_type: row.get(3)?,
                    link_mode: row.get(4)?,
                    exists,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for attachment in &mut rows {
            attachment.title = self.field_values(attachment.id)?.remove("title");
        }
        Ok(rows)
    }

    pub fn list_collections(&self) -> Result<Vec<CollectionInfo>> {
        if !self.table_exists("collections") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT c.collectionID, c.key, c.collectionName, c.parentCollectionID,
                    COUNT(ci.itemID) AS item_count
             FROM collections c
             LEFT JOIN collectionItems ci ON c.collectionID = ci.collectionID
             GROUP BY c.collectionID, c.key, c.collectionName, c.parentCollectionID
             ORDER BY c.collectionName COLLATE NOCASE ASC",
        )?;
        let collections = stmt
            .query_map([], |row| {
                Ok(CollectionInfo {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    name: row.get(2)?,
                    parent_id: row.get(3)?,
                    item_count: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(collections)
    }

    pub fn collection_items(&self, collection_key: &str, limit: usize) -> Result<Vec<ItemSummary>> {
        if !self.table_exists("collections") || !self.table_exists("collectionItems") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT i.itemID, i.key, COALESCE(it.typeName, 'unknown'), i.dateAdded, i.dateModified
             FROM collectionItems ci
             JOIN collections c ON ci.collectionID = c.collectionID
             JOIN items i ON ci.itemID = i.itemID
             LEFT JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
             WHERE c.key = ?
             ORDER BY i.dateModified DESC
             LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![collection_key, limit as i64], row_to_base_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| self.enrich_summary(row))
            .collect()
    }

    pub fn list_tags(&self) -> Result<Vec<TagInfo>> {
        if !self.table_exists("tags") || !self.table_exists("itemTags") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT t.name, COUNT(it.itemID) AS item_count
             FROM tags t
             JOIN itemTags it ON t.tagID = it.tagID
             GROUP BY t.name
             ORDER BY item_count DESC, t.name COLLATE NOCASE ASC",
        )?;
        let tags = stmt
            .query_map([], |row| {
                Ok(TagInfo {
                    name: row.get(0)?,
                    item_count: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(tags)
    }

    pub fn tag_items(&self, tag: &str, limit: usize) -> Result<Vec<ItemSummary>> {
        if !self.table_exists("tags") || !self.table_exists("itemTags") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT i.itemID, i.key, COALESCE(it.typeName, 'unknown'), i.dateAdded, i.dateModified
             FROM itemTags itag
             JOIN tags t ON itag.tagID = t.tagID
             JOIN items i ON itag.itemID = i.itemID
             LEFT JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
             WHERE t.name = ?
             ORDER BY i.dateModified DESC
             LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![tag, limit as i64], row_to_base_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| self.enrich_summary(row))
            .collect()
    }

    pub fn bibtex(&self, key: &str) -> Result<String> {
        let detail = self.get_item(key)?;
        let item = detail.summary;
        let entry_type = match item.item_type.as_str() {
            "book" => "book",
            "conferencePaper" => "inproceedings",
            _ => "article",
        };
        let mut fields = Vec::new();
        push_bib_field(&mut fields, "title", item.title.as_deref());
        if !item.authors.is_empty() {
            fields.push(format!("  author = {{{}}}", item.authors.join(" and ")));
        }
        push_bib_field(
            &mut fields,
            "year",
            item.year.map(|y| y.to_string()).as_deref(),
        );
        push_bib_field(&mut fields, "doi", item.doi.as_deref());
        push_bib_field(&mut fields, "url", item.url.as_deref());
        let entry_key = item.citation_key.as_deref().unwrap_or(&item.key);
        Ok(format!(
            "@{entry_type}{{{},\n{}\n}}",
            entry_key,
            fields.join(",\n")
        ))
    }

    fn find_lfz_full_md(&self, config: &Config, detail: &ItemDetail) -> Result<Option<PathBuf>> {
        let Some(zotero_data_dir) = config.lfz.zotero_data_dir.as_ref() else {
            return Ok(None);
        };
        let cache_root = zotero_data_dir.join("llm-for-zotero-mineru");
        let mut candidates = Vec::new();
        for attachment in detail.attachments.iter().filter(|attachment| {
            is_pdf_attachment(attachment) || attachment.resolved_path.is_none()
        }) {
            let attachment_id = attachment.id.to_string();
            candidates.push(cache_root.join(&attachment_id).join("full.md"));
            candidates.push(cache_root.join(&attachment_id).join("_content.md"));
            candidates.push(cache_root.join(format!("{attachment_id}.md")));
        }
        dedup_paths(&mut candidates);
        Ok(candidates
            .into_iter()
            .find(|path| path.is_file() && markdown_cache_filename(path)))
    }

    pub fn reading_recap(
        &self,
        range: &DateRange,
        state_dir: Option<&Path>,
    ) -> Result<Vec<ReadingEntry>> {
        let mut entries = BTreeMap::<String, ReadingEntry>::new();

        for record in activity::read_records(state_dir, range)? {
            let Some(key) = record.item_key.as_deref() else {
                continue;
            };
            let item_id = record
                .item_id
                .or_else(|| self.item_id_by_key(key).ok().flatten());
            let Some(item_id) = item_id else {
                continue;
            };
            let mut entry = self.reading_entry_for_item(item_id, record.ts, "cli_read_log")?;
            entry.timestamp = record.ts;
            entries.insert(format!("cli:{item_id}:{}", record.ts), entry);
        }

        for (item_id, timestamp, provenance) in self.recap_source_events(range)? {
            let key = format!("{provenance}:{item_id}:{timestamp}");
            entries.entry(key).or_insert(self.reading_entry_for_item(
                item_id,
                timestamp,
                &provenance,
            )?);
        }

        let mut output = entries.into_values().collect::<Vec<_>>();
        output.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(output)
    }

    pub fn log_read(&self, config: &Config, command: &str, item: &ItemSummary) {
        activity::append_read(
            config.state_dir.as_deref(),
            command,
            Some(&item.key),
            Some(item.id),
            item.title.as_deref(),
        );
    }

    fn base_items(&self, limit: usize) -> Result<Vec<ItemSummary>> {
        let deleted_filter = if self.table_exists("deletedItems") {
            "AND NOT EXISTS (SELECT 1 FROM deletedItems d WHERE d.itemID = i.itemID)"
        } else {
            ""
        };
        let sql = format!(
            "SELECT i.itemID, i.key, COALESCE(it.typeName, 'unknown'), i.dateAdded, i.dateModified
             FROM items i
             LEFT JOIN itemTypes it ON i.itemTypeID = it.itemTypeID
             WHERE COALESCE(it.typeName, 'unknown') NOT IN ('attachment', 'note', 'annotation')
             {deleted_filter}
             ORDER BY i.dateModified DESC, i.dateAdded DESC
             LIMIT ?"
        );
        let limit_value = if limit == usize::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([limit_value], row_to_base_item)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| self.enrich_summary(row))
            .collect()
    }

    fn enrich_summary(&self, base: BaseItemRow) -> Result<ItemSummary> {
        let fields = self.field_values(base.id)?;
        let title = fields.get("title").cloned();
        let short_title = fields.get("shortTitle").cloned();
        let date = fields.get("date").or_else(|| fields.get("year")).cloned();
        let year = parse_year(date.as_deref());
        let doi = fields.get("DOI").or_else(|| fields.get("doi")).cloned();
        let url = fields.get("url").cloned();
        let extra = fields.get("extra");
        let citation_key = fields
            .get("citationKey")
            .cloned()
            .or_else(|| extract_extra_citation_key(extra.map(String::as_str)));
        let arxiv = extract_arxiv_id(
            extra
                .or(url.as_ref())
                .or(doi.as_ref())
                .or(title.as_ref())
                .map(String::as_str),
        );
        Ok(ItemSummary {
            id: base.id,
            key: base.key,
            item_type: base.item_type,
            title,
            short_title,
            citation_key,
            authors: self.creators_for_item(base.id)?,
            year,
            doi,
            arxiv,
            url,
            date_added: base.date_added,
            date_modified: base.date_modified,
        })
    }

    fn field_values(&self, item_id: i64) -> Result<BTreeMap<String, String>> {
        if !self.table_exists("itemData")
            || !self.table_exists("fields")
            || !self.table_exists("itemDataValues")
        {
            return Ok(BTreeMap::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT f.fieldName, idv.value
             FROM itemData id
             JOIN fields f ON id.fieldID = f.fieldID
             JOIN itemDataValues idv ON id.valueID = idv.valueID
             WHERE id.itemID = ?",
        )?;
        let rows = stmt.query_map([item_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = BTreeMap::new();
        for row in rows {
            let (key, value) = row?;
            map.insert(key, value);
        }
        Ok(map)
    }

    fn creators_for_item(&self, item_id: i64) -> Result<Vec<String>> {
        if !self.table_exists("itemCreators") || !self.table_exists("creators") {
            return Ok(Vec::new());
        }
        let sql = if self.table_exists("creatorData")
            && self.column_exists("creators", "creatorDataID")
        {
            "SELECT cd.firstName, cd.lastName, cd.shortName, cd.fieldMode
             FROM itemCreators ic
             JOIN creators c ON ic.creatorID = c.creatorID
             JOIN creatorData cd ON c.creatorDataID = cd.creatorDataID
             WHERE ic.itemID = ?
             ORDER BY ic.orderIndex"
        } else if self.column_exists("creators", "firstName") {
            "SELECT c.firstName, c.lastName, NULL, c.fieldMode
             FROM itemCreators ic
             JOIN creators c ON ic.creatorID = c.creatorID
             WHERE ic.itemID = ?
             ORDER BY ic.orderIndex"
        } else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([item_id], |row| {
            let first: Option<String> = row.get(0)?;
            let last: Option<String> = row.get(1)?;
            let short: Option<String> = row.get(2)?;
            let field_mode: Option<i64> = row.get(3)?;
            Ok(format_creator(first, last, short, field_mode))
        })?;
        Ok(rows
            .filter_map(|row| row.ok())
            .filter(|value| !value.trim().is_empty())
            .collect())
    }

    fn tags_for_item(&self, item_id: i64) -> Result<Vec<String>> {
        if !self.table_exists("tags") || !self.table_exists("itemTags") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT t.name
             FROM tags t
             JOIN itemTags it ON t.tagID = it.tagID
             WHERE it.itemID = ?
             ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([item_id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn collections_for_item(&self, item_id: i64) -> Result<Vec<CollectionInfo>> {
        if !self.table_exists("collections") || !self.table_exists("collectionItems") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT c.collectionID, c.key, c.collectionName, c.parentCollectionID
             FROM collectionItems ci
             JOIN collections c ON ci.collectionID = c.collectionID
             WHERE ci.itemID = ?
             ORDER BY c.collectionName COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([item_id], |row| {
                Ok(CollectionInfo {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    name: row.get(2)?,
                    parent_id: row.get(3)?,
                    item_count: None,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn read_attachment_text(
        &self,
        attachment: &AttachmentInfo,
    ) -> Result<Option<(String, String)>> {
        let Some(path) = &attachment.resolved_path else {
            return Ok(None);
        };
        let mut candidates = Vec::new();
        if path.is_dir() {
            candidates.push(path.join(".zotero-ft-cache"));
            candidates.push(path.join(".zotero-ft-unprocessed"));
        } else if let Some(parent) = path.parent() {
            candidates.push(parent.join(".zotero-ft-cache"));
            candidates.push(parent.join(".zotero-ft-unprocessed"));
            if is_probably_text_file(path, attachment.content_type.as_deref()) {
                candidates.push(path.clone());
            }
        }
        for candidate in candidates {
            if candidate.exists() && candidate.is_file() {
                let text = fs::read_to_string(&candidate)
                    .with_context(|| format!("failed to read {}", candidate.display()))?;
                if !text.trim().is_empty() {
                    return Ok(Some((text, candidate.display().to_string())));
                }
            }
        }
        Ok(None)
    }

    fn resolve_attachment_path(
        &self,
        attachment_key: &str,
        raw_path: Option<&str>,
    ) -> Option<PathBuf> {
        let raw_path = raw_path?;
        if let Some(rest) = raw_path.strip_prefix("storage:") {
            let filename = rest.trim_start_matches('/');
            return self
                .storage_path
                .as_ref()
                .map(|storage| storage.join(attachment_key).join(filename));
        }
        if raw_path.starts_with("attachments:") {
            return None;
        }
        let path = PathBuf::from(raw_path);
        if path.is_absolute() {
            Some(path)
        } else {
            self.storage_path
                .as_ref()
                .and_then(|storage| storage.parent().map(|parent| parent.join(path)))
        }
    }

    fn recap_source_events(&self, range: &DateRange) -> Result<Vec<(i64, i64, String)>> {
        let mut events = Vec::new();
        for item in self.base_items(usize::MAX)? {
            let mut provenances = HashSet::new();
            for annotation in self.annotations_for_item(item.id)? {
                if let Some(ts) = annotation
                    .date_modified
                    .as_deref()
                    .and_then(timestamp_millis_from_str)
                {
                    if range.contains_millis(ts) && provenances.insert("annotation") {
                        events.push((item.id, ts, "annotation".to_string()));
                    }
                }
            }
            for note in self.notes_for_item(item.id)? {
                if let Some(ts) = note
                    .date_modified
                    .as_deref()
                    .and_then(timestamp_millis_from_str)
                {
                    if range.contains_millis(ts) && provenances.insert("note") {
                        events.push((item.id, ts, "note".to_string()));
                    }
                }
            }
            if let Some(ts) = item
                .date_modified
                .as_deref()
                .and_then(timestamp_millis_from_str)
            {
                if range.contains_millis(ts) {
                    events.push((item.id, ts, "metadata_modified".to_string()));
                }
            }
        }
        Ok(events)
    }

    fn reading_entry_for_item(
        &self,
        item_id: i64,
        timestamp: i64,
        provenance: &str,
    ) -> Result<ReadingEntry> {
        let detail = self.item_detail_by_id(item_id)?;
        Ok(ReadingEntry {
            item: detail.summary,
            collections: detail.collections,
            tags: detail.tags,
            attachments: detail.attachments,
            annotation_count: detail.annotation_count,
            note_count: detail.note_count,
            timestamp,
            timestamp_iso: DateTime::<Utc>::from_timestamp_millis(timestamp)
                .map(|dt| dt.to_rfc3339()),
            provenance: provenance.to_string(),
        })
    }
}

#[derive(Debug)]
struct BaseItemRow {
    id: i64,
    key: String,
    item_type: String,
    date_added: Option<String>,
    date_modified: Option<String>,
}

fn row_to_base_item(row: &Row<'_>) -> rusqlite::Result<BaseItemRow> {
    Ok(BaseItemRow {
        id: row.get(0)?,
        key: row.get(1)?,
        item_type: row.get(2)?,
        date_added: row.get(3)?,
        date_modified: row.get(4)?,
    })
}

fn sqlite_readonly_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut escaped = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b' ' => escaped.push_str("%20"),
            b'#' => escaped.push_str("%23"),
            b'?' => escaped.push_str("%3F"),
            b'%' => escaped.push_str("%25"),
            _ => escaped.push(byte as char),
        }
    }
    format!("file:{escaped}?mode=ro&immutable=1")
}

fn sql_value_to_string(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    let value = row.get_ref(index)?;
    Ok(match value {
        rusqlite::types::ValueRef::Null => None,
        rusqlite::types::ValueRef::Integer(v) => Some(v.to_string()),
        rusqlite::types::ValueRef::Real(v) => Some(v.to_string()),
        rusqlite::types::ValueRef::Text(v) => Some(String::from_utf8_lossy(v).to_string()),
        rusqlite::types::ValueRef::Blob(_) => None,
    })
}

fn first_existing_column(db: &ZoteroDb, table: &str, candidates: &[&str]) -> String {
    candidates
        .iter()
        .find(|candidate| db.column_exists(table, candidate))
        .map(|column| format!("ia.{column}"))
        .unwrap_or_else(|| "NULL".to_string())
}

fn is_safe_ident(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn format_creator(
    first: Option<String>,
    last: Option<String>,
    short: Option<String>,
    field_mode: Option<i64>,
) -> String {
    if field_mode == Some(1) {
        return short.or(last).or(first).unwrap_or_default();
    }
    match (
        first.filter(|s| !s.is_empty()),
        last.filter(|s| !s.is_empty()),
    ) {
        (Some(first), Some(last)) => format!("{first} {last}"),
        (None, Some(last)) => last,
        (Some(first), None) => first,
        (None, None) => short.unwrap_or_default(),
    }
}

fn parse_year(value: Option<&str>) -> Option<i32> {
    let re = Regex::new(r"(19|20)\d{2}").ok()?;
    re.find(value?)?.as_str().parse().ok()
}

#[derive(Clone, Copy)]
struct FieldScore {
    exact: i64,
    contains: i64,
    token_weight: i64,
    all_token_bonus: i64,
}

impl FieldScore {
    fn new(exact: i64, contains: i64, token_weight: i64, all_token_bonus: i64) -> Self {
        Self {
            exact,
            contains,
            token_weight,
            all_token_bonus,
        }
    }
}

fn score_text_field(
    name: &str,
    value: Option<&str>,
    query: &str,
    needle: &str,
    tokens: &[String],
    weights: FieldScore,
    score: &mut i64,
    reasons: &mut Vec<String>,
    matched: &mut BTreeMap<String, String>,
) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let lower = value.to_lowercase();
    if value.eq_ignore_ascii_case(query) {
        *score = (*score).max(weights.exact);
        reasons.push(format!("{name}_exact"));
        matched.insert(name.to_string(), compact_match_value(value));
        return;
    }
    if lower.contains(needle) {
        *score = (*score).max(weights.contains);
        reasons.push(format!("{name}_contains"));
        matched.insert(name.to_string(), compact_match_value(value));
    }
    if !tokens.is_empty() && weights.token_weight > 0 {
        let count = tokens
            .iter()
            .filter(|token| lower.contains(token.as_str()))
            .count();
        if count > 0 {
            let token_score = count as i64 * weights.token_weight
                + if count == tokens.len() {
                    weights.all_token_bonus
                } else {
                    0
                };
            *score = (*score).max(token_score);
            reasons.push(format!("{name}_tokens"));
            matched
                .entry(name.to_string())
                .or_insert_with(|| compact_match_value(value));
        }
    }
}

fn score_acronym_field(
    name: &str,
    value: Option<&str>,
    needle: &str,
    exact_score: i64,
    contains_score: i64,
    score: &mut i64,
    reasons: &mut Vec<String>,
    matched: &mut BTreeMap<String, String>,
) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let acronym = text_acronym(value).to_lowercase();
    if acronym.len() < 2 {
        return;
    }
    if acronym == needle {
        *score = (*score).max(exact_score);
        reasons.push(format!("{name}_acronym_exact"));
        matched.insert(name.to_string(), compact_match_value(value));
    } else if acronym.contains(needle) {
        *score = (*score).max(contains_score);
        reasons.push(format!("{name}_acronym_contains"));
        matched.insert(name.to_string(), compact_match_value(value));
    }
}

fn search_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
    {
        let token = token.to_lowercase();
        if !tokens.iter().any(|existing| existing == &token) {
            tokens.push(token);
        }
    }
    tokens
}

fn text_acronym(value: &str) -> String {
    let mut acronym = String::new();
    for token in value
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        if let Some(ch) = token.chars().next() {
            acronym.extend(ch.to_uppercase());
        }
    }
    acronym
}

fn non_empty(value: &str) -> Option<&str> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn compact_match_value(value: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= 240 {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

pub fn extract_arxiv_id(value: Option<&str>) -> Option<String> {
    let text = value?;
    let re = Regex::new(
        r"(?i)(?:arxiv[:\s/]*|abs/)(\d{4}\.\d{4,5}(?:v\d+)?|[a-z-]+(?:\.[A-Z]{2})?/\d{7}(?:v\d+)?)",
    )
    .ok()?;
    re.captures(text)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

pub fn extract_extra_citation_key(value: Option<&str>) -> Option<String> {
    let value = value?;
    for line in value.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        for prefix in ["citation key:", "citationkey:"] {
            if lower.starts_with(prefix) {
                let key = trimmed[prefix.len()..].trim();
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }
    None
}

fn render_fallback_markdown(detail: &ItemDetail, extracted: &ExtractedText) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("key: {}\n", yaml_string(&detail.summary.key)));
    out.push_str(&format!(
        "title: {}\n",
        yaml_string(detail.summary.title.as_deref().unwrap_or("Untitled"))
    ));
    yaml_opt(
        &mut out,
        "short_title",
        detail.summary.short_title.as_deref(),
    );
    yaml_opt(
        &mut out,
        "citation_key",
        detail.summary.citation_key.as_deref(),
    );
    yaml_string_list(&mut out, "authors", &detail.summary.authors);
    if let Some(year) = detail.summary.year {
        out.push_str(&format!("year: {year}\n"));
    }
    yaml_opt(&mut out, "doi", detail.summary.doi.as_deref());
    yaml_opt(&mut out, "arxiv", detail.summary.arxiv.as_deref());
    yaml_opt(&mut out, "url", detail.summary.url.as_deref());
    yaml_string_list(
        &mut out,
        "collections",
        &detail
            .collections
            .iter()
            .map(|collection| collection.name.clone())
            .collect::<Vec<_>>(),
    );
    yaml_string_list(&mut out, "tags", &detail.tags);
    out.push_str("source: zcli_fallback\n");
    out.push_str(&format!("extracted_truncated: {}\n", extracted.truncated));
    out.push_str("---\n\n");

    out.push_str(&format!(
        "# {}\n\n",
        detail.summary.title.as_deref().unwrap_or("Untitled")
    ));
    push_metadata_section(&mut out, detail);

    if let Some(abstract_note) = detail.fields.get("abstractNote") {
        push_section(&mut out, "Abstract", abstract_note);
    }

    if detail.note_count > 0 {
        out.push_str("## Notes\n\n");
        out.push_str(&format!(
            "{} note(s) are available via `zcli item notes {}`.\n\n",
            detail.note_count, detail.summary.key
        ));
    }

    if detail.annotation_count > 0 {
        out.push_str("## Annotations\n\n");
        out.push_str(&format!(
            "{} annotation(s) are available via `zcli item annotations {}`.\n\n",
            detail.annotation_count, detail.summary.key
        ));
    }

    if !detail.attachments.is_empty() {
        out.push_str("## Attachments\n\n");
        for attachment in &detail.attachments {
            let label = attachment
                .title
                .as_deref()
                .or(attachment.path.as_deref())
                .unwrap_or(&attachment.key);
            out.push_str(&format!(
                "- {} ({})\n",
                label,
                if attachment.exists {
                    "available"
                } else {
                    "missing"
                }
            ));
        }
        out.push('\n');
    }

    push_section(&mut out, "Extracted Text", &extracted.text);
    out
}

fn push_metadata_section(out: &mut String, detail: &ItemDetail) {
    out.push_str("## Metadata\n\n");
    out.push_str(&format!("- Key: `{}`\n", detail.summary.key));
    if let Some(citation_key) = &detail.summary.citation_key {
        out.push_str(&format!("- Citation Key: `{citation_key}`\n"));
    }
    if let Some(short_title) = &detail.summary.short_title {
        out.push_str(&format!("- Short Title: {short_title}\n"));
    }
    if !detail.summary.authors.is_empty() {
        out.push_str(&format!(
            "- Authors: {}\n",
            detail.summary.authors.join(", ")
        ));
    }
    if let Some(year) = detail.summary.year {
        out.push_str(&format!("- Year: {year}\n"));
    }
    if let Some(doi) = &detail.summary.doi {
        out.push_str(&format!("- DOI: {doi}\n"));
    }
    if let Some(arxiv) = &detail.summary.arxiv {
        out.push_str(&format!("- arXiv: {arxiv}\n"));
    }
    if let Some(url) = &detail.summary.url {
        out.push_str(&format!("- URL: {url}\n"));
    }
    out.push('\n');
}

fn push_section(out: &mut String, title: &str, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    out.push_str(&format!("## {title}\n\n"));
    out.push_str(body.trim());
    out.push_str("\n\n");
}

fn yaml_opt(out: &mut String, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        out.push_str(&format!("{key}: {}\n", yaml_string(value)));
    }
}

fn yaml_string_list(out: &mut String, key: &str, values: &[String]) {
    if values.is_empty() {
        out.push_str(&format!("{key}: []\n"));
    } else {
        out.push_str(&format!("{key}:\n"));
        for value in values {
            out.push_str(&format!("  - {}\n", yaml_string(value)));
        }
    }
}

fn yaml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
}

fn markdown_cache_filename(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let lower = name.to_ascii_lowercase();
            lower == "full.md" || lower == "_content.md" || lower.ends_with(".md")
        })
        .unwrap_or(false)
}

fn is_pdf_attachment(attachment: &AttachmentInfo) -> bool {
    attachment
        .content_type
        .as_deref()
        .map(|content_type| content_type.eq_ignore_ascii_case("application/pdf"))
        .unwrap_or(false)
        || attachment
            .resolved_path
            .as_deref()
            .and_then(|path| path.extension())
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
}

pub fn parse_db_datetime(value: &str) -> Option<DateTime<Utc>> {
    let trimmed = value.trim();
    DateTime::parse_from_rfc3339(trimmed)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| Utc.from_utc_datetime(&dt))
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .map(|dt| Utc.from_utc_datetime(&dt))
        })
}

pub fn timestamp_millis_from_str(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if let Ok(number) = trimmed.parse::<i64>() {
        return Some(if number > 10_000_000_000 {
            number
        } else {
            number * 1000
        });
    }
    parse_db_datetime(trimmed).map(|dt| dt.timestamp_millis())
}

fn strip_html(value: &str) -> String {
    let tag_re = Regex::new(r"<[^>]+>").ok();
    let text = if let Some(tag_re) = tag_re {
        tag_re.replace_all(value, " ").into_owned()
    } else {
        value.to_string()
    };
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_probably_text_file(path: &Path, content_type: Option<&str>) -> bool {
    content_type
        .map(|mime| mime.starts_with("text/") || mime.contains("json") || mime.contains("xml"))
        .unwrap_or(false)
        || path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "txt" | "md" | "json" | "html" | "xml"
                )
            })
            .unwrap_or(false)
}

fn snippet_at(text: &str, byte_offset: usize, width: usize) -> String {
    let start = text[..byte_offset.min(text.len())]
        .char_indices()
        .rev()
        .nth(width / 2)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let end = text[byte_offset.min(text.len())..]
        .char_indices()
        .nth(width)
        .map(|(idx, _)| byte_offset.min(text.len()) + idx)
        .unwrap_or(text.len());
    text[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_bib_field(fields: &mut Vec<String>, name: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        let escaped = value.replace('{', "\\{").replace('}', "\\}");
        fields.push(format!("  {name} = {{{escaped}}}"));
    }
}
