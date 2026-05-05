use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::{mutation::MutationPlan, zotero::ZoteroDb};

pub struct PlanOptions<'a> {
    pub collections: &'a [String],
    pub tags: &'a [String],
    pub allow_duplicates: bool,
}

struct NormalizedIdentifier {
    kind: String,
    value: String,
    exact_score: i64,
}

struct NormalizedPdfSource {
    kind: String,
    value: String,
    duplicate_query: String,
    exists: bool,
}

pub fn identifiers(
    inputs: &[String],
    forced_kind: Option<&str>,
    command_name: &str,
    db: Option<&ZoteroDb>,
    options: &PlanOptions<'_>,
) -> Result<MutationPlan> {
    if inputs.is_empty() {
        return Err(anyhow!("pass at least one identifier"));
    }
    let mut plan = Vec::new();
    let mut helper_identifiers = Vec::new();
    for input in inputs {
        let identifier = normalize_identifier(input, forced_kind)?;
        let existing = existing_matches(db, &identifier.value, identifier.exact_score);
        let skipped = !options.allow_duplicates && !existing.is_empty();
        if !skipped {
            helper_identifiers.push(json!({
                "input": input,
                "kind": identifier.kind,
                "value": identifier.value,
            }));
        }
        plan.push(json!({
            "input": input,
            "kind": identifier.kind,
            "value": identifier.value,
            "status": if skipped { "skip_existing" } else { "import" },
            "existing_matches": existing,
        }));
    }
    Ok(MutationPlan::new(
        "import_identifiers",
        json!({
        "identifiers": helper_identifiers,
        "collections": options.collections,
        "tags": options.tags,
        "allowDuplicates": options.allow_duplicates,
        "saveAttachments": true,
        }),
        json!({
            "mode": "paper_identifier_import",
            "sources": plan,
            "collections": options.collections,
            "tags": options.tags,
            "allow_duplicates": options.allow_duplicates,
            "duplicate_check": duplicate_check_label(db),
            "zotero_native_path": "Zotero.Translate.Search / Add Item by Identifier",
            "execute_command": format!("zcli import {command_name} <identifiers...> --execute"),
        }),
    )
    .with_empty_execute_reason("all requested sources matched existing local Zotero items"))
}

pub fn pdfs(
    inputs: &[String],
    recognize: bool,
    db: Option<&ZoteroDb>,
    options: &PlanOptions<'_>,
) -> Result<MutationPlan> {
    if inputs.is_empty() {
        return Err(anyhow!("pass at least one PDF path or URL"));
    }
    let mut plan = Vec::new();
    let mut helper_sources = Vec::new();
    for input in inputs {
        let source = normalize_pdf_source(input);
        let existing = existing_matches(db, &source.duplicate_query, 85);
        let skipped = !options.allow_duplicates && !existing.is_empty();
        if !skipped {
            helper_sources.push(match source.kind.as_str() {
                "pdf_url" => json!({ "url": source.value }),
                _ => json!({ "path": source.value }),
            });
        }
        plan.push(json!({
            "input": input,
            "kind": source.kind,
            "value": source.value,
            "exists": source.exists,
            "status": if skipped {
                "skip_existing"
            } else if source.kind == "local_pdf" && !source.exists {
                "not_found"
            } else {
                "import"
            },
            "existing_matches": existing,
        }));
    }
    Ok(
        MutationPlan::new(
            "import_pdfs",
            json!({
                "sources": helper_sources,
                "collections": options.collections,
                "tags": options.tags,
                "recognize": recognize,
            }),
            json!({
                "mode": "paper_pdf_import",
                "sources": plan,
                "collections": options.collections,
                "tags": options.tags,
                "recognize_metadata": recognize,
                "allow_duplicates": options.allow_duplicates,
                "duplicate_check": duplicate_check_label(db),
                "zotero_native_path": "Zotero.Attachments.importFromFile/importFromURL + Zotero.RecognizeDocument",
                "execute_command": "zcli import pdf <paths-or-urls...> --execute",
            }),
        )
        .with_empty_execute_reason("all requested sources matched existing local Zotero items"),
    )
}

pub fn urls(
    inputs: &[String],
    db: Option<&ZoteroDb>,
    options: &PlanOptions<'_>,
) -> Result<MutationPlan> {
    if inputs.is_empty() {
        return Err(anyhow!("pass at least one URL"));
    }
    let mut plan = Vec::new();
    let mut helper_urls = Vec::new();
    for input in inputs {
        let url = input.trim();
        if url.is_empty() {
            continue;
        }
        let identifier = normalize_identifier(url, None).ok();
        let (kind, query, exact_score) = identifier
            .as_ref()
            .map(|identifier| {
                (
                    identifier.kind.clone(),
                    identifier.value.clone(),
                    identifier.exact_score,
                )
            })
            .unwrap_or_else(|| {
                (
                    if is_probably_pdf_url(url) {
                        "pdf_url".to_string()
                    } else {
                        "web_url".to_string()
                    },
                    url.to_string(),
                    90,
                )
            });
        let existing = existing_matches(db, &query, exact_score);
        let skipped = !options.allow_duplicates && !existing.is_empty();
        if !skipped {
            helper_urls.push(url.to_string());
        }
        plan.push(json!({
            "input": input,
            "kind": kind,
            "normalized": query,
            "status": if skipped { "skip_existing" } else { "import" },
            "existing_matches": existing,
        }));
    }
    Ok(
        MutationPlan::new(
            "import_urls",
            json!({
                "urls": helper_urls,
                "collections": options.collections,
                "tags": options.tags,
            }),
            json!({
                "mode": "paper_url_import",
                "sources": plan,
                "collections": options.collections,
                "tags": options.tags,
                "allow_duplicates": options.allow_duplicates,
                "duplicate_check": duplicate_check_label(db),
                "zotero_native_path": "identifier translator, PDF import/recognition, or web translator fallback",
                "execute_command": "zcli import url <urls...> --execute",
            }),
        )
        .with_empty_execute_reason("all requested sources matched existing local Zotero items"),
    )
}

pub fn alphaxiv_zotero_plan(paper: &Value) -> Value {
    let alpha_id = paper
        .get("alphaxiv_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let canonical_id = paper.get("canonical_id").and_then(Value::as_str);
    let url = paper
        .get("url")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if alpha_id.is_empty() {
                "https://www.alphaxiv.org".to_string()
            } else {
                format!("https://www.alphaxiv.org/abs/{alpha_id}")
            }
        });

    let mut commands = Vec::new();
    let import_strategy;
    if let Some(base_id) = arxiv_base_id(alpha_id).or_else(|| canonical_id.and_then(arxiv_base_id))
    {
        import_strategy = "arxiv";
        commands.push(format!(
            "zcli import arxiv {} --dry-run --format json",
            shell_quote(&base_id)
        ));
    } else if let Some(canonical_id) = canonical_id {
        import_strategy = "pdf";
        let pdf_path = format!("/tmp/{canonical_id}.pdf");
        let id_for_pdf = if alpha_id.is_empty() {
            canonical_id
        } else {
            alpha_id
        };
        commands.push(format!(
            "zcli alphaxiv pdf {} --download {} --format json",
            shell_quote(id_for_pdf),
            shell_quote(&pdf_path)
        ));
        commands.push(format!(
            "zcli import pdf {} --dry-run --format json",
            shell_quote(&pdf_path)
        ));
        commands.push(format!(
            "zcli import url {} --dry-run --format json",
            shell_quote(&url)
        ));
    } else {
        import_strategy = "url";
        commands.push(format!(
            "zcli import url {} --dry-run --format json",
            shell_quote(&url)
        ));
    }

    json!({
        "import_strategy": import_strategy,
        "dry_run_commands": commands,
    })
}

fn duplicate_check_label(db: Option<&ZoteroDb>) -> &'static str {
    if db.is_some() {
        "local_zotero_db"
    } else {
        "unavailable"
    }
}

fn normalize_identifier(input: &str, forced_kind: Option<&str>) -> Result<NormalizedIdentifier> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(anyhow!("empty identifier"));
    }
    if forced_kind == Some("arxiv") {
        return Ok(NormalizedIdentifier {
            kind: "arxiv".to_string(),
            value: normalize_arxiv_id(raw)?,
            exact_score: 94,
        });
    }
    if let Ok(arxiv) = normalize_arxiv_id(raw) {
        return Ok(NormalizedIdentifier {
            kind: "arxiv".to_string(),
            value: arxiv,
            exact_score: 94,
        });
    }
    if let Some(doi) = normalize_doi(raw) {
        return Ok(NormalizedIdentifier {
            kind: "doi".to_string(),
            value: doi,
            exact_score: 95,
        });
    }
    Ok(NormalizedIdentifier {
        kind: "identifier".to_string(),
        value: raw.to_string(),
        exact_score: 90,
    })
}

fn normalize_arxiv_id(input: &str) -> Result<String> {
    let raw = input.trim();
    let url_re = regex::Regex::new(
        r"(?i)arxiv\.org/(?:abs|pdf)/([0-9]{4}\.[0-9]{4,5}(?:v[0-9]+)?|[a-z-]+(?:\.[A-Z]{2})?/[0-9]{7}(?:v[0-9]+)?)(?:\.pdf)?",
    )?;
    if let Some(captures) = url_re.captures(raw) {
        return Ok(captures[1].to_string());
    }
    let prefixed = regex::Regex::new(r"(?i)^arxiv[:\s]+(.+)$")?;
    let candidate = prefixed
        .captures(raw)
        .map(|captures| captures[1].trim().to_string())
        .unwrap_or_else(|| raw.to_string());
    let candidate = candidate.trim_end_matches(".pdf");
    let plain = regex::Regex::new(
        r"(?i)^([0-9]{4}\.[0-9]{4,5}(?:v[0-9]+)?|[a-z-]+(?:\.[A-Z]{2})?/[0-9]{7}(?:v[0-9]+)?)$",
    )?;
    if plain.is_match(candidate) {
        Ok(candidate.to_string())
    } else {
        Err(anyhow!("not an arXiv identifier: {input}"))
    }
}

fn normalize_doi(input: &str) -> Option<String> {
    let mut raw = input.trim();
    if let Some(rest) = raw
        .strip_prefix("https://doi.org/")
        .or_else(|| raw.strip_prefix("http://doi.org/"))
        .or_else(|| raw.strip_prefix("https://dx.doi.org/"))
        .or_else(|| raw.strip_prefix("http://dx.doi.org/"))
    {
        raw = rest;
    }
    let re = regex::Regex::new(r#"(?i)\b(10\.[0-9]{4,9}/[^\s"'<>{}]+[^\s"'<>{}.,;:)])"#).ok()?;
    re.captures(raw).map(|captures| captures[1].to_string())
}

fn normalize_pdf_source(input: &str) -> NormalizedPdfSource {
    let raw = input.trim();
    if is_probably_http_url(raw) {
        return NormalizedPdfSource {
            kind: "pdf_url".to_string(),
            value: raw.to_string(),
            duplicate_query: raw.to_string(),
            exists: true,
        };
    }
    let path = PathBuf::from(raw);
    let canonical = path.canonicalize().unwrap_or(path);
    let exists = canonical.exists();
    NormalizedPdfSource {
        kind: "local_pdf".to_string(),
        value: canonical.display().to_string(),
        duplicate_query: canonical.display().to_string(),
        exists,
    }
}

fn existing_matches(db: Option<&ZoteroDb>, query: &str, exact_score: i64) -> Vec<Value> {
    let Some(db) = db else {
        return Vec::new();
    };
    db.resolve_items(query, 5)
        .unwrap_or_default()
        .into_iter()
        .filter(|value| {
            value
                .get("score")
                .and_then(Value::as_i64)
                .map(|score| score >= exact_score)
                .unwrap_or(false)
        })
        .collect()
}

fn is_probably_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn is_probably_pdf_url(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.contains("/pdf/") || lower.ends_with(".pdf") || lower.contains(".pdf?")
}

fn arxiv_base_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let base = value
        .rsplit_once('v')
        .and_then(|(base, version)| {
            version
                .chars()
                .all(|ch| ch.is_ascii_digit())
                .then_some(base)
        })
        .unwrap_or(value);
    is_arxiv_base_id(base).then(|| base.to_string())
}

fn is_arxiv_base_id(value: &str) -> bool {
    let Some((left, right)) = value.split_once('.') else {
        return false;
    };
    left.len() == 4
        && left.chars().all(|ch| ch.is_ascii_digit())
        && (4..=5).contains(&right.len())
        && right.chars().all(|ch| ch.is_ascii_digit())
}

fn shell_quote(input: &str) -> String {
    if input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:".contains(c))
    {
        input.to_string()
    } else {
        format!("'{}'", input.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_arxiv_identifier_imports() {
        let inputs = vec!["https://arxiv.org/abs/2604.25850v3".to_string()];
        let plan = identifiers(
            &inputs,
            Some("arxiv"),
            "arxiv",
            None,
            &PlanOptions {
                collections: &[],
                tags: &[],
                allow_duplicates: false,
            },
        )
        .unwrap();
        assert_eq!(plan.helper_op, "import_identifiers");
        assert_eq!(plan.preview["sources"][0]["value"], "2604.25850v3");
        assert_eq!(plan.preview["duplicate_check"], "unavailable");
        assert_eq!(plan.params["identifiers"][0]["kind"], "arxiv");
    }

    #[test]
    fn plans_pdf_and_url_sources() {
        let inputs = vec!["https://example.com/paper.pdf".to_string()];
        let plan = pdfs(
            &inputs,
            true,
            None,
            &PlanOptions {
                collections: &[],
                tags: &[],
                allow_duplicates: false,
            },
        )
        .unwrap();
        assert_eq!(plan.helper_op, "import_pdfs");
        assert_eq!(plan.preview["sources"][0]["kind"], "pdf_url");
        assert_eq!(plan.params["sources"][0]["url"], inputs[0]);
    }

    #[test]
    fn plans_alphaxiv_handoff_to_arxiv() {
        let paper = json!({
            "alphaxiv_id": "2604.25850",
            "canonical_id": "2604.25850v3",
            "url": "https://www.alphaxiv.org/abs/2604.25850"
        });
        let plan = alphaxiv_zotero_plan(&paper);
        assert_eq!(plan["import_strategy"], "arxiv");
        assert_eq!(
            plan["dry_run_commands"][0],
            "zcli import arxiv 2604.25850 --dry-run --format json"
        );
    }
}
