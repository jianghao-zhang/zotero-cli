use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::{config::Config, zotero::ZoteroDb};

#[derive(Clone, Copy, Debug)]
pub enum MaterializeMode {
    Hardlink,
    Copy,
}

impl MaterializeMode {
    fn label(self) -> &'static str {
        match self {
            Self::Hardlink => "hardlink",
            Self::Copy => "copy",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReadOptions {
    pub output_dir: PathBuf,
    pub mode: MaterializeMode,
}

pub fn prepare(config: &Config, query: &str, options: &ReadOptions) -> Result<Value> {
    let db = ZoteroDb::open(config)?;
    let query = query.trim();
    if query.is_empty() {
        return Err(anyhow!("paper query must not be empty"));
    }

    let (detail, resolution) = match db.get_item(query) {
        Ok(detail) => (detail, json!({"kind": "item_key"})),
        Err(_) => {
            let matches = db.resolve_items(query, 5)?;
            let Some(first) = matches.first() else {
                return Ok(json!({
                    "ok": false,
                    "schema": "zotero_reading_surface/v1",
                    "query": query,
                    "reason": "paper_not_found",
                }));
            };
            let first_score = first.get("score").and_then(Value::as_i64).unwrap_or(0);
            let tied = matches
                .get(1)
                .and_then(|candidate| candidate.get("score"))
                .and_then(Value::as_i64)
                == Some(first_score);
            if tied {
                return Ok(json!({
                    "ok": false,
                    "schema": "zotero_reading_surface/v1",
                    "query": query,
                    "reason": "ambiguous_query",
                    "candidates": matches,
                }));
            }
            let key = first
                .pointer("/item/key")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("resolved paper is missing its Zotero key"))?;
            (
                db.get_item(key)?,
                json!({
                    "kind": "query",
                    "score": first_score,
                    "reasons": first.get("reasons").cloned().unwrap_or_else(|| json!([])),
                }),
            )
        }
    };

    let markdown_status = db.markdown_status(config, &detail.summary.key)?;
    let markdown_source = markdown_status
        .get("preferred_source")
        .and_then(Value::as_str)
        .unwrap_or("zcli_fallback");
    let markdown_source_path = markdown_status
        .pointer("/selected/path")
        .cloned()
        .unwrap_or(Value::Null);
    let context_command = format!(
        "zcli context {} --budget 40k --format json",
        detail.summary.key
    );

    let pdf = detail.attachments.iter().find(|attachment| {
        attachment.exists
            && attachment.resolved_path.is_some()
            && (attachment.content_type.as_deref() == Some("application/pdf")
                || attachment
                    .resolved_path
                    .as_deref()
                    .and_then(Path::extension)
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")))
    });
    let Some(pdf) = pdf else {
        return Ok(json!({
            "ok": false,
            "schema": "zotero_reading_surface/v1",
            "query": query,
            "item": detail.summary,
            "reason": "local_pdf_not_found",
            "attachments": detail.attachments,
        }));
    };
    let source = pdf.resolved_path.as_deref().expect("checked above");
    verify_pdf(source)?;

    let output_dir = absolute_path(&options.output_dir)?;
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;
    let filename = preview_filename(
        detail.summary.title.as_deref().unwrap_or("Untitled paper"),
        &detail.summary.key,
    );
    let preview_path = output_dir.join(filename);
    let materialization = materialize(source, &preview_path, options.mode)?;
    let preview_path = fs::canonicalize(&preview_path).unwrap_or(preview_path);
    let source_path = fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let preview_markdown = format!("[Open PDF](<{}>)", preview_path.display());

    db.log_read(config, "read", &detail.summary);
    Ok(json!({
        "ok": true,
        "schema": "zotero_reading_surface/v1",
        "query": query,
        "resolution": resolution,
        "item": detail.summary,
        "reading": {
            "primary": "markdown",
            "markdown": {
                "source": markdown_source,
                "source_path": markdown_source_path,
                "native_markdown_available": markdown_status.get("has_lfz_markdown").cloned().unwrap_or(Value::Bool(false)),
                "command": context_command,
            },
            "policy": "Read Markdown first. Use the PDF only as a visual preview and for figures or layout that Markdown cannot preserve.",
        },
        "pdf": {
            "role": "visual_preview",
            "attachment_key": pdf.key,
            "mime_type": "application/pdf",
            "source_path": source_path,
            "preview_path": preview_path,
            "preview_markdown": preview_markdown.clone(),
            "materialization": materialization,
            "bytes": fs::metadata(source)?.len(),
        },
        "response": {
            "open_pdf_markdown": preview_markdown,
            "required_in_paper_reading_final": true,
            "repeat_while_same_paper_is_active": true,
            "placement": "end_of_final_answer",
        },
        "evidence": {
            "context": context_command,
            "passages": format!("zcli index chunks \"QUERY\" --item {} --format json", detail.summary.key),
            "annotations": format!("zcli item annotations {} --format json", detail.summary.key),
            "notes": format!("zcli item notes {} --format json", detail.summary.key),
        },
    }))
}

fn verify_pdf(path: &Path) -> Result<()> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut magic = [0_u8; 5];
    file.read_exact(&mut magic)
        .with_context(|| format!("failed to read PDF header from {}", path.display()))?;
    if &magic != b"%PDF-" {
        return Err(anyhow!(
            "attachment is labeled as PDF but has no PDF header: {}",
            path.display()
        ));
    }
    Ok(())
}

fn materialize(source: &Path, target: &Path, mode: MaterializeMode) -> Result<&'static str> {
    if target.exists() {
        let source_len = fs::metadata(source)?.len();
        let target_len = fs::metadata(target)?.len();
        if source_len == target_len {
            return Ok("existing");
        }
        return Err(anyhow!(
            "preview target already exists with different contents: {}",
            target.display()
        ));
    }

    match mode {
        MaterializeMode::Hardlink => fs::hard_link(source, target).with_context(|| {
            format!(
                "failed to hardlink {} to {}; retry with --copy only when the files are on different volumes",
                source.display(),
                target.display()
            )
        })?,
        MaterializeMode::Copy => {
            fs::copy(source, target).with_context(|| {
                format!("failed to copy {} to {}", source.display(), target.display())
            })?;
        }
    }
    Ok(mode.label())
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn preview_filename(title: &str, key: &str) -> String {
    let mut safe = title
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '-',
            character if character.is_control() => ' ',
            character => character,
        })
        .collect::<String>();
    safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    if safe.chars().count() > 120 {
        safe = safe.chars().take(120).collect::<String>();
    }
    format!("{} — {}.pdf", safe.trim(), key)
}
