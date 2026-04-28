use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

use crate::{config::Config, zotero::ZoteroDb};

const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub include_full_text: bool,
    pub max_chars: usize,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    pub snippet_chars: usize,
}

#[derive(Debug, Clone)]
pub struct GetOptions {
    pub max_chars: usize,
}

pub fn index_path(config: &Config) -> Result<PathBuf> {
    config
        .cache_dir
        .clone()
        .ok_or_else(|| anyhow!("cache_dir is not configured"))
        .map(|dir| dir.join("index.sqlite"))
}

pub fn status(config: &Config) -> Result<Value> {
    let path = index_path(config)?;
    let exists = path.exists();
    let mut value = json!({
        "ok": true,
        "index_path": path,
        "exists": exists,
        "schema_version": SCHEMA_VERSION,
        "backend": "sqlite_fts5_bm25",
        "network_required": false,
        "models_required": false,
        "future_layers": ["local_embedding", "local_reranker", "daemon"],
    });
    if exists {
        let conn = open_readonly(&path)
            .with_context(|| format!("failed to open index {}", path.display()))?;
        value["document_count"] = json!(table_count(&conn, "documents")?);
        value["last_updated_at"] = json!(meta_value(&conn, "last_updated_at")?);
        value["source_db_path"] = json!(meta_value(&conn, "source_db_path")?);
        value["include_full_text"] = json!(meta_value(&conn, "include_full_text")?);
    }
    Ok(value)
}

pub fn update(config: &Config, options: &IndexOptions) -> Result<Value> {
    let path = index_path(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(&path)
        .with_context(|| format!("failed to open index {}", path.display()))?;
    configure_connection(&conn)?;
    ensure_schema(&conn)?;

    let zotero = ZoteroDb::open(config)?;
    let items = zotero.list_items(None, usize::MAX)?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM documents_fts", [])?;
    tx.execute("DELETE FROM documents", [])?;

    let mut indexed = 0_usize;
    for item in items {
        let detail = zotero.item_detail_by_id(item.id)?;
        let abstract_text = detail
            .fields
            .get("abstractNote")
            .cloned()
            .unwrap_or_default();
        let notes_text = zotero
            .notes_for_item(detail.summary.id)?
            .into_iter()
            .filter_map(|note| note.text)
            .collect::<Vec<_>>()
            .join("\n\n");
        let annotations_text = zotero
            .annotations_for_item(detail.summary.id)?
            .into_iter()
            .filter_map(|annotation| {
                let text = [annotation.text, annotation.comment]
                    .into_iter()
                    .flatten()
                    .filter(|value| !value.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.trim().is_empty()).then_some(text)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let body_text = if options.include_full_text {
            zotero
                .extract_text(&detail.summary.key, options.max_chars)?
                .text
        } else {
            String::new()
        };
        let authors_text = detail.summary.authors.join("; ");
        let tags_text = detail.tags.join("; ");
        let collections_text = detail
            .collections
            .iter()
            .map(|collection| collection.name.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let identifiers = [
            detail.summary.key.as_str(),
            detail.summary.citation_key.as_deref().unwrap_or_default(),
            detail.summary.doi.as_deref().unwrap_or_default(),
            detail.summary.arxiv.as_deref().unwrap_or_default(),
            detail.summary.url.as_deref().unwrap_or_default(),
        ]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
        let aliases = search_aliases(&[
            detail.summary.citation_key.as_deref(),
            detail.summary.short_title.as_deref(),
            detail.summary.title.as_deref(),
        ]);
        let item_json = serde_json::to_string(&detail.summary)?;
        let tags_json = serde_json::to_string(&detail.tags)?;
        let collections_json = serde_json::to_string(&detail.collections)?;
        let content_chars = count_chars(&[
            abstract_text.as_str(),
            notes_text.as_str(),
            annotations_text.as_str(),
            body_text.as_str(),
        ]);

        tx.execute(
            "INSERT INTO documents
             (item_id, key, citation_key, short_title, title, item_json, authors_text, year,
              doi, arxiv, url, tags_text, collections_text, tags_json, collections_json,
              abstract_text, notes_text, annotations_text, body_text, content_chars, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                detail.summary.id,
                detail.summary.key,
                detail.summary.citation_key,
                detail.summary.short_title,
                detail.summary.title,
                item_json,
                authors_text,
                detail.summary.year,
                detail.summary.doi,
                detail.summary.arxiv,
                detail.summary.url,
                tags_text,
                collections_text,
                tags_json,
                collections_json,
                abstract_text,
                notes_text,
                annotations_text,
                body_text,
                content_chars as i64,
                Utc::now().to_rfc3339(),
            ],
        )?;
        tx.execute(
            "INSERT INTO documents_fts
             (rowid, key, citation_key, short_title, title, authors, year, identifiers, aliases,
              tags, collections, abstract, notes, annotations, body)
             SELECT item_id, key, COALESCE(citation_key, ''), COALESCE(short_title, ''),
                    COALESCE(title, ''), authors_text, COALESCE(year, ''), ?2, ?3,
                    tags_text, collections_text, abstract_text, notes_text, annotations_text, body_text
             FROM documents WHERE item_id = ?1",
            params![detail.summary.id, identifiers, aliases],
        )?;
        indexed += 1;
    }

    set_meta_tx(&tx, "schema_version", &SCHEMA_VERSION.to_string())?;
    set_meta_tx(&tx, "last_updated_at", &Utc::now().to_rfc3339())?;
    set_meta_tx(
        &tx,
        "source_db_path",
        &config
            .zotero_db_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    )?;
    set_meta_tx(
        &tx,
        "include_full_text",
        if options.include_full_text {
            "true"
        } else {
            "false"
        },
    )?;
    set_meta_tx(&tx, "max_chars", &options.max_chars.to_string())?;
    tx.commit()?;

    Ok(json!({
        "ok": true,
        "index_path": path,
        "backend": "sqlite_fts5_bm25",
        "strategy": "full_rebuild_local_sidecar",
        "indexed": indexed,
        "include_full_text": options.include_full_text,
        "max_chars": options.max_chars,
        "network_required": false,
        "models_required": false,
    }))
}

pub fn search(config: &Config, query: &str, options: &SearchOptions) -> Result<Value> {
    let path = index_path(config)?;
    if !path.exists() {
        anyhow::bail!(
            "local index does not exist: {}; run zcli index update first",
            path.display()
        );
    }
    let conn =
        open_readonly(&path).with_context(|| format!("failed to open index {}", path.display()))?;
    let fts_query = build_fts_query(query)?;
    let mut stmt = conn.prepare(
        "SELECT d.item_json, d.tags_json, d.collections_json, d.content_chars,
                bm25(documents_fts, 9.0, 9.0, 8.0, 7.0, 4.0, 1.0, 8.0, 7.5, 4.0, 4.0, 3.0, 2.0, 2.0, 1.0) AS rank,
                snippet(documents_fts, -1, '[', ']', '...', ?2) AS snippet
         FROM documents_fts
         JOIN documents d ON d.item_id = documents_fts.rowid
         WHERE documents_fts MATCH ?1
         ORDER BY rank
         LIMIT ?3",
    )?;
    let candidate_limit = options.limit.saturating_mul(5).clamp(options.limit, 50);
    let rows = stmt
        .query_map(
            params![
                fts_query,
                snippet_token_count(options.snippet_chars),
                candidate_limit as i64,
            ],
            |row| {
                let item_json: String = row.get(0)?;
                let tags_json: String = row.get(1)?;
                let collections_json: String = row.get(2)?;
                let content_chars: i64 = row.get(3)?;
                let rank: f64 = row.get(4)?;
                let snippet: String = row.get(5)?;
                Ok((
                    item_json,
                    tags_json,
                    collections_json,
                    content_chars,
                    rank,
                    snippet,
                ))
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut hits = Vec::new();
    for (item_json, tags_json, collections_json, content_chars, rank, snippet) in rows {
        let item = serde_json::from_str::<Value>(&item_json)?;
        let rank_score = bm25_score(rank);
        let bonus = rerank_bonus(&item, query);
        hits.push(json!({
            "item": item,
            "tags": serde_json::from_str::<Value>(&tags_json)?,
            "collections": serde_json::from_str::<Value>(&collections_json)?,
            "rank": rank,
            "score": rank_score + bonus,
            "score_components": {
                "bm25": rank_score,
                "local_bonus": bonus,
            },
            "snippet": snippet,
            "content_chars": content_chars,
        }));
    }
    hits.sort_by(|a, b| {
        json_score(b)
            .partial_cmp(&json_score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(options.limit);

    Ok(json!({
        "ok": true,
        "query": query,
        "query_fts": build_fts_query(query)?,
        "backend": "sqlite_fts5_bm25",
        "index_path": path,
        "hits": hits,
    }))
}

pub fn get(config: &Config, key: &str, options: &GetOptions) -> Result<Value> {
    let path = index_path(config)?;
    if !path.exists() {
        anyhow::bail!(
            "local index does not exist: {}; run zcli index update first",
            path.display()
        );
    }
    let conn =
        open_readonly(&path).with_context(|| format!("failed to open index {}", path.display()))?;
    let doc = conn
        .query_row(
            "SELECT item_json, tags_json, collections_json, abstract_text, notes_text,
                    annotations_text, body_text, content_chars, indexed_at
             FROM documents
             WHERE key = ?1 OR citation_key = ?1 OR short_title = ?1
             ORDER BY CASE WHEN key = ?1 THEN 0 WHEN citation_key = ?1 THEN 1 ELSE 2 END
             LIMIT 1",
            [key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("indexed item not found: {key}"))?;

    let (
        item_json,
        tags_json,
        collections_json,
        abstract_text,
        notes_text,
        annotations_text,
        body_text,
        content_chars,
        indexed_at,
    ) = doc;
    Ok(json!({
        "ok": true,
        "index_path": path,
        "item": serde_json::from_str::<Value>(&item_json)?,
        "tags": serde_json::from_str::<Value>(&tags_json)?,
        "collections": serde_json::from_str::<Value>(&collections_json)?,
        "abstract": truncate_chars(&abstract_text, options.max_chars),
        "notes": truncate_chars(&notes_text, options.max_chars),
        "annotations": truncate_chars(&annotations_text, options.max_chars),
        "body": truncate_chars(&body_text, options.max_chars),
        "content_chars": content_chars,
        "indexed_at": indexed_at,
        "truncated": {
            "abstract": abstract_text.chars().count() > options.max_chars,
            "notes": notes_text.chars().count() > options.max_chars,
            "annotations": annotations_text.chars().count() > options.max_chars,
            "body": body_text.chars().count() > options.max_chars,
        }
    }))
}

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn open_readonly(path: &Path) -> Result<Connection> {
    let uri = sqlite_readonly_uri(path);
    Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .or_else(|_| Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY))
    .map_err(Into::into)
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

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute("DROP TABLE IF EXISTS documents_fts", [])?;
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS index_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS documents (
            item_id INTEGER PRIMARY KEY,
            key TEXT NOT NULL UNIQUE,
            citation_key TEXT,
            short_title TEXT,
            title TEXT,
            item_json TEXT NOT NULL,
            authors_text TEXT NOT NULL,
            year INTEGER,
            doi TEXT,
            arxiv TEXT,
            url TEXT,
            tags_text TEXT NOT NULL,
            collections_text TEXT NOT NULL,
            tags_json TEXT NOT NULL,
            collections_json TEXT NOT NULL,
            abstract_text TEXT NOT NULL,
            notes_text TEXT NOT NULL,
            annotations_text TEXT NOT NULL,
            body_text TEXT NOT NULL,
            content_chars INTEGER NOT NULL,
            indexed_at TEXT NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
            key,
            citation_key,
            short_title,
            title,
            authors,
            year,
            identifiers,
            aliases,
            tags,
            collections,
            abstract,
            notes,
            annotations,
            body
        );
        "#,
    )?;
    Ok(())
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    if !is_safe_ident(table) {
        anyhow::bail!("unsafe table name");
    }
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .map_err(Into::into)
}

fn meta_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM index_meta WHERE key = ?", [key], |row| {
        row.get(0)
    })
    .optional()
    .map_err(Into::into)
}

fn set_meta_tx(tx: &rusqlite::Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO index_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )?;
    Ok(())
}

fn build_fts_query(query: &str) -> Result<String> {
    let tokens = fts_tokens(query);
    if tokens.is_empty() {
        anyhow::bail!("empty search query");
    }
    let mut parts = Vec::new();
    if tokens.len() > 1 {
        parts.push(format!("\"{}\"", escape_fts_phrase(&tokens.join(" "))));
    }
    parts.extend(
        tokens
            .iter()
            .map(|token| format!("\"{}\"", escape_fts_phrase(token))),
    );
    Ok(parts.join(" OR "))
}

fn fts_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for token in query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let token = token.to_lowercase();
        if !tokens.iter().any(|existing| existing == &token) {
            tokens.push(token);
        }
    }
    tokens
}

fn search_aliases(values: &[Option<&str>]) -> String {
    let mut aliases = Vec::new();
    for value in values.iter().flatten() {
        for alias in [split_camel_alias(value), acronym_alias(value)] {
            if alias.chars().count() >= 2 && !aliases.iter().any(|existing| existing == &alias) {
                aliases.push(alias);
            }
        }
    }
    aliases.join("\n")
}

fn split_camel_alias(value: &str) -> String {
    let mut out = String::new();
    let chars = value.chars().collect::<Vec<_>>();
    for (idx, ch) in chars.iter().enumerate() {
        if idx > 0 {
            let prev = chars[idx - 1];
            let next = chars.get(idx + 1).copied();
            let boundary = (prev.is_ascii_lowercase() && ch.is_ascii_uppercase())
                || (prev.is_ascii_alphabetic() && ch.is_ascii_digit())
                || (prev.is_ascii_digit() && ch.is_ascii_alphabetic())
                || (prev.is_ascii_uppercase()
                    && ch.is_ascii_uppercase()
                    && next.map(|next| next.is_ascii_lowercase()).unwrap_or(false));
            if boundary {
                out.push(' ');
            }
        }
        if ch.is_alphanumeric() {
            out.push(*ch);
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn acronym_alias(value: &str) -> String {
    let acronym = text_acronym(value);
    if acronym.chars().count() >= 2 {
        acronym
    } else {
        String::new()
    }
}

fn escape_fts_phrase(value: &str) -> String {
    value.replace('"', "\"\"")
}

fn snippet_token_count(chars: usize) -> i64 {
    (chars / 6).clamp(12, 64) as i64
}

fn rerank_bonus(item: &Value, query: &str) -> f64 {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return 0.0;
    }
    let mut bonus = 0.0;
    if item
        .get("citation_key")
        .and_then(Value::as_str)
        .map(|value| value.eq_ignore_ascii_case(query))
        .unwrap_or(false)
    {
        bonus += 0.22;
    }
    if item
        .get("short_title")
        .and_then(Value::as_str)
        .map(|value| value.eq_ignore_ascii_case(query))
        .unwrap_or(false)
    {
        bonus += 0.20;
    }
    if item
        .get("title")
        .and_then(Value::as_str)
        .map(|value| text_acronym(value).to_lowercase().starts_with(&needle))
        .unwrap_or(false)
    {
        bonus += 0.16;
    }
    if item
        .get("title")
        .and_then(Value::as_str)
        .map(|value| value.to_lowercase().contains(&needle))
        .unwrap_or(false)
    {
        bonus += 0.04;
    }
    if item
        .get("item_type")
        .and_then(Value::as_str)
        .map(is_paper_like_type)
        .unwrap_or(false)
    {
        bonus += 0.22;
    }
    if item
        .get("item_type")
        .and_then(Value::as_str)
        .map(|item_type| item_type == "forumPost")
        .unwrap_or(false)
    {
        bonus -= 0.55;
    }
    if item
        .get("url")
        .and_then(Value::as_str)
        .map(is_social_url)
        .unwrap_or(false)
    {
        bonus -= 0.35;
    }
    bonus
}

fn bm25_score(rank: f64) -> f64 {
    if rank < 0.0 {
        -rank
    } else {
        1.0 / (1.0 + rank)
    }
}

fn json_score(value: &Value) -> f64 {
    value.get("score").and_then(Value::as_f64).unwrap_or(0.0)
}

fn is_paper_like_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "journalArticle" | "conferencePaper" | "preprint" | "book" | "bookSection" | "thesis"
    )
}

fn is_social_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("://x.com/")
        || lower.contains("://twitter.com/")
        || lower.contains("://mobile.twitter.com/")
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

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

fn count_chars(values: &[&str]) -> usize {
    values.iter().map(|value| value.chars().count()).sum()
}

fn is_safe_ident(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
