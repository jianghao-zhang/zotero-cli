use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use clap::ValueEnum;
use serde_json::{json, Value};

use crate::{config::Config, zotero::ZoteroDb};

const INDEX_FILE: &str = ".zcli-mirror-index.json";

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MirrorMode {
    Symlink,
    Copy,
}

#[derive(Clone, Copy, Debug)]
pub struct MirrorOptions {
    pub dry_run: bool,
    pub mode: MirrorMode,
    pub limit: usize,
    pub incremental: bool,
    pub cleanup_stale: bool,
    pub write_markdown: bool,
    pub markdown_max_chars: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct MirrorWatchOptions {
    pub interval_secs: u64,
    pub settle_ms: u64,
    pub include_storage: bool,
    pub once: bool,
    pub run_on_start: bool,
    pub dry_run: bool,
    pub mode: MirrorMode,
    pub limit: usize,
    pub cleanup_stale: bool,
    pub max_events: Option<usize>,
    pub write_markdown: bool,
    pub markdown_max_chars: usize,
}

pub fn status(config: &Config) -> Result<Value> {
    let root = config.mirror_root.clone();
    let index_path = root.as_ref().map(|root| root.join(INDEX_FILE));
    let safety = root
        .as_ref()
        .map(|root| mirror_root_safety(config, root))
        .transpose()?;
    Ok(json!({
        "ok": true,
        "mirror_root": root,
        "configured": config.mirror_root.is_some(),
        "root_exists": config.mirror_root.as_deref().map(Path::exists).unwrap_or(false),
        "index_path": index_path,
        "index_exists": index_path.as_deref().map(Path::exists).unwrap_or(false),
        "safety": safety,
        "modes": ["symlink", "copy"],
        "markdown": {
            "file": "paper.md",
            "enable_with": "--write-markdown",
            "source_order": ["llm_for_zotero_full_md", "zcli_fallback"],
        },
        "auto_update": {
            "real_time": false,
            "foreground_watcher": "zcli mirror watch",
            "manual_refresh": ["zcli mirror rebuild", "zcli mirror sync"],
            "default_interval_secs": 60,
            "default_settle_ms": 5000,
            "default_signature": "zotero_db_only",
        },
    }))
}

pub fn rebuild(config: &Config, db: &ZoteroDb, options: MirrorOptions) -> Result<Value> {
    let root = config.mirror_root.as_ref().ok_or_else(|| {
        anyhow!("mirror_root is not configured; pass --mirror-root or set it in config")
    })?;
    ensure_safe_root(config, root)?;

    let old_dirs = read_index_dirs(root)?;
    let mut planned = Vec::new();
    let mut planned_dirs = HashSet::new();
    let items = db.list_items(None, options.limit)?;
    for item in items {
        let detail = db.get_item(&item.key)?;
        let slug = item_slug(&detail.summary);
        let collections = if detail.collections.is_empty() {
            vec!["Unfiled".to_string()]
        } else {
            detail
                .collections
                .iter()
                .map(|collection| safe_name(&collection.name))
                .collect::<Vec<_>>()
        };

        let allin_dir = root.join("Allin").join(&slug);
        planned_dirs.insert(allin_dir.clone());
        planned.push(write_item_dir(
            config, db, &allin_dir, &detail, options, "Allin", root,
        )?);

        for collection in collections {
            let item_dir = root.join(&collection).join(&slug);
            planned_dirs.insert(item_dir.clone());
            planned.push(write_item_dir(
                config,
                db,
                &item_dir,
                &detail,
                options,
                &collection,
                root,
            )?);
        }
    }

    let index = json!({
        "version": 1,
        "mode": format!("{:?}", options.mode).to_ascii_lowercase(),
        "incremental": options.incremental,
        "cleanup_stale": options.cleanup_stale,
        "entries": planned,
    });
    let stale = stale_dirs(&old_dirs, &planned_dirs);
    let mut removed_stale = Vec::new();
    if !options.dry_run {
        fs::create_dir_all(root)?;
        if options.cleanup_stale {
            removed_stale = remove_stale_dirs(root, &stale)?;
        }
        write_if_changed(
            &root.join(INDEX_FILE),
            &serde_json::to_string_pretty(&index)?,
        )?;
    }

    Ok(json!({
        "ok": true,
        "dry_run": options.dry_run,
        "mirror_root": root,
        "index_path": root.join(INDEX_FILE),
        "planned_count": index["entries"].as_array().map(Vec::len).unwrap_or(0),
        "stale_count": stale.len(),
        "stale_removed": removed_stale,
        "index": index,
    }))
}

pub fn watch(config: &Config, options: MirrorWatchOptions) -> Result<Value> {
    let root = config.mirror_root.as_ref().ok_or_else(|| {
        anyhow!("mirror_root is not configured; pass --mirror-root or set it in config")
    })?;
    ensure_safe_root(config, root)?;
    let mut last_signature = input_signature(config, options.include_storage)?;
    let mut events = Vec::new();
    let mut sync_count = 0usize;

    if options.run_on_start || options.once {
        let event = run_watch_sync(config, options, "startup", &last_signature)?;
        events.push(event);
        sync_count += 1;
        if options.once || reached_max(options.max_events, sync_count) {
            return Ok(watch_result(root, options, last_signature, events));
        }
    }

    eprintln!(
        "zcli mirror watch: watching {} every {}s; press Ctrl-C to stop",
        root.display(),
        options.interval_secs
    );

    loop {
        thread::sleep(Duration::from_secs(options.interval_secs.max(1)));
        let current_signature = input_signature(config, options.include_storage)?;
        if current_signature == last_signature {
            continue;
        }

        thread::sleep(Duration::from_millis(options.settle_ms));
        let settled_signature = input_signature(config, options.include_storage)?;
        if settled_signature == last_signature {
            continue;
        }

        let event = run_watch_sync(config, options, "change_detected", &settled_signature)?;
        eprintln!("zcli mirror watch: synced at {}", Utc::now().to_rfc3339());
        last_signature = settled_signature;
        events.push(event);
        sync_count += 1;
        if reached_max(options.max_events, sync_count) {
            return Ok(watch_result(root, options, last_signature, events));
        }
    }
}

fn ensure_safe_root(config: &Config, root: &Path) -> Result<()> {
    let safety = mirror_root_safety(config, root)?;
    if safety["ok"].as_bool().unwrap_or(false) {
        return Ok(());
    }
    let warnings = safety["warnings"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_else(|| "mirror_root overlaps Zotero-managed paths".to_string());
    anyhow::bail!(
        "unsafe mirror_root: {warnings}. Choose a directory outside Zotero data/storage."
    );
}

fn mirror_root_safety(config: &Config, root: &Path) -> Result<Value> {
    let root = comparable_path(root)?;
    let mut warnings = Vec::new();

    if let Some(db_path) = &config.zotero_db_path {
        let db_path = comparable_path(db_path)?;
        if root == db_path {
            warnings.push(format!(
                "mirror_root points at Zotero database file {}",
                db_path.display()
            ));
        }
        if let Some(data_dir) = db_path.parent() {
            if root == data_dir || root.starts_with(data_dir) {
                warnings.push(format!(
                    "mirror_root is inside Zotero data directory {}",
                    data_dir.display()
                ));
            }
        }
    }

    if let Some(storage_path) = &config.zotero_storage_path {
        let storage_path = comparable_path(storage_path)?;
        if root == storage_path || root.starts_with(&storage_path) {
            warnings.push(format!(
                "mirror_root is inside Zotero storage directory {}",
                storage_path.display()
            ));
        }
    }

    Ok(json!({
        "ok": warnings.is_empty(),
        "root": root,
        "warnings": warnings,
    }))
}

fn comparable_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", path.display()));
    }
    if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
        let parent = parent
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", parent.display()))?;
        return Ok(path
            .file_name()
            .map(|name| parent.join(name))
            .unwrap_or(parent));
    }
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(std::env::current_dir()?.join(path))
}

fn write_item_dir(
    config: &Config,
    db: &ZoteroDb,
    item_dir: &Path,
    detail: &crate::zotero::ItemDetail,
    options: MirrorOptions,
    collection_label: &str,
    root: &Path,
) -> Result<Value> {
    let metadata_path = item_dir.join("metadata.json");
    let notes_dir = item_dir.join("notes");
    let attachments_dir = item_dir.join("attachments");
    let arxiv_path = item_dir.join("arxiv.id");
    let markdown_path = item_dir.join("paper.md");
    let mut attachment_outputs = Vec::new();
    let mut markdown_output = Value::Null;

    if !options.dry_run {
        ensure_dir(&notes_dir)?;
        ensure_dir(&attachments_dir)?;
        write_if_changed(&metadata_path, &serde_json::to_string_pretty(detail)?)?;
        if let Some(arxiv) = &detail.summary.arxiv {
            write_if_changed(&arxiv_path, arxiv)?;
        }
    }

    for attachment in &detail.attachments {
        let Some(source) = attachment
            .resolved_path
            .as_ref()
            .filter(|path| path.exists())
        else {
            continue;
        };
        let filename = source
            .file_name()
            .and_then(|name| name.to_str())
            .map(safe_name)
            .unwrap_or_else(|| attachment.key.clone());
        let target = attachments_dir.join(filename);
        if !options.dry_run {
            match options.mode {
                MirrorMode::Symlink => link_file(source, &target)?,
                MirrorMode::Copy => {
                    copy_file_if_changed(source, &target)?;
                }
            }
        }
        attachment_outputs.push(json!({
            "source": source,
            "target": target,
            "mode": format!("{:?}", options.mode).to_ascii_lowercase(),
        }));
    }

    if options.write_markdown {
        let markdown = db.markdown_for_item(
            config,
            &detail.summary.key,
            options.markdown_max_chars,
            true,
        )?;
        if !options.dry_run {
            write_if_changed(&markdown_path, &markdown.markdown)?;
        }
        markdown_output = json!({
            "target": markdown_path,
            "source": markdown.source,
            "source_path": markdown.source_path,
            "fallback_used": markdown.fallback_used,
            "extracted_truncated": markdown.extracted_truncated,
            "chars": markdown.markdown.chars().count(),
        });
    }

    Ok(json!({
        "item_key": detail.summary.key,
        "title": detail.summary.title,
        "collection": collection_label,
        "dir": item_dir,
        "relative_dir": item_dir.strip_prefix(root).ok(),
        "metadata": metadata_path,
        "markdown": markdown_output,
        "arxiv": detail.summary.arxiv,
        "attachments": attachment_outputs,
    }))
}

fn run_watch_sync(
    config: &Config,
    options: MirrorWatchOptions,
    reason: &str,
    signature: &Value,
) -> Result<Value> {
    let db = ZoteroDb::open(config)?;
    let result = rebuild(
        config,
        &db,
        MirrorOptions {
            dry_run: options.dry_run,
            mode: options.mode,
            limit: options.limit,
            incremental: true,
            cleanup_stale: options.cleanup_stale,
            write_markdown: options.write_markdown,
            markdown_max_chars: options.markdown_max_chars,
        },
    )?;
    Ok(json!({
        "reason": reason,
        "at": Utc::now().to_rfc3339(),
        "signature": signature,
        "result": {
            "ok": result.get("ok").and_then(Value::as_bool).unwrap_or(false),
            "dry_run": result.get("dry_run").and_then(Value::as_bool).unwrap_or(false),
            "planned_count": result.get("planned_count").and_then(Value::as_u64).unwrap_or(0),
            "stale_count": result.get("stale_count").and_then(Value::as_u64).unwrap_or(0),
            "stale_removed": result.get("stale_removed").cloned().unwrap_or_else(|| json!([])),
        }
    }))
}

fn watch_result(
    root: &Path,
    options: MirrorWatchOptions,
    final_signature: Value,
    events: Vec<Value>,
) -> Value {
    json!({
        "ok": true,
        "mirror_root": root,
        "watch": {
            "interval_secs": options.interval_secs,
            "settle_ms": options.settle_ms,
            "include_storage": options.include_storage,
            "dry_run": options.dry_run,
            "mode": format!("{:?}", options.mode).to_ascii_lowercase(),
            "cleanup_stale": options.cleanup_stale,
            "foreground": true,
        },
        "final_signature": final_signature,
        "events": events,
    })
}

fn reached_max(max_events: Option<usize>, sync_count: usize) -> bool {
    max_events
        .map(|max_events| sync_count >= max_events)
        .unwrap_or(false)
}

fn input_signature(config: &Config, include_storage: bool) -> Result<Value> {
    let storage = if include_storage {
        config
            .zotero_storage_path
            .as_ref()
            .map(|path| path_signature(path))
            .transpose()?
    } else {
        None
    };
    Ok(json!({
        "include_storage": include_storage,
        "zotero_db": config.zotero_db_path.as_ref().map(|path| path_signature(path)).transpose()?,
        "zotero_storage": storage,
    }))
}

fn path_signature(path: &Path) -> Result<Value> {
    let metadata = fs::metadata(path).ok();
    let modified_ms = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    Ok(json!({
        "path": path,
        "exists": path.exists(),
        "modified_ms": modified_ms,
        "len": metadata.map(|metadata| metadata.len()),
    }))
}

fn ensure_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(path)?;
    Ok(())
}

fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::write(path, contents)?;
    Ok(())
}

fn copy_file_if_changed(source: &Path, target: &Path) -> Result<()> {
    if same_file_signature(source, target)? {
        return Ok(());
    }
    fs::copy(source, target)?;
    Ok(())
}

fn same_file_signature(source: &Path, target: &Path) -> Result<bool> {
    let Ok(source_metadata) = fs::metadata(source) else {
        return Ok(false);
    };
    let Ok(target_metadata) = fs::metadata(target) else {
        return Ok(false);
    };
    if source_metadata.len() != target_metadata.len() {
        return Ok(false);
    }
    let source_modified = source_metadata.modified().ok();
    let target_modified = target_metadata.modified().ok();
    Ok(matches!(
        (source_modified, target_modified),
        (Some(source_modified), Some(target_modified)) if target_modified >= source_modified
    ))
}

fn read_index_dirs(root: &Path) -> Result<HashSet<PathBuf>> {
    let path = root.join(INDEX_FILE);
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw = fs::read_to_string(path)?;
    let parsed: Value = serde_json::from_str(&raw)?;
    let mut dirs = HashSet::new();
    if let Some(entries) = parsed.get("entries").and_then(Value::as_array) {
        for entry in entries {
            let Some(raw_dir) = entry.get("dir").and_then(Value::as_str) else {
                continue;
            };
            let dir = PathBuf::from(raw_dir);
            dirs.insert(if dir.is_absolute() {
                dir
            } else {
                root.join(dir)
            });
        }
    }
    Ok(dirs)
}

fn stale_dirs(old_dirs: &HashSet<PathBuf>, planned_dirs: &HashSet<PathBuf>) -> Vec<PathBuf> {
    old_dirs
        .difference(planned_dirs)
        .cloned()
        .collect::<Vec<_>>()
}

fn remove_stale_dirs(root: &Path, stale: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for dir in stale {
        if !is_generated_item_dir(root, dir) {
            continue;
        }
        if dir.exists() {
            fs::remove_dir_all(dir)?;
            removed.push(dir.clone());
            prune_empty_parents(dir.parent(), root)?;
        }
    }
    Ok(removed)
}

fn is_generated_item_dir(root: &Path, dir: &Path) -> bool {
    dir != root && dir.starts_with(root) && dir.join("metadata.json").exists()
}

fn prune_empty_parents(mut current: Option<&Path>, root: &Path) -> Result<()> {
    while let Some(dir) = current {
        if dir == root || !dir.starts_with(root) {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
    Ok(())
}

#[cfg(unix)]
fn link_file(source: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    if target.exists() {
        if fs::read_link(target)
            .map(|existing| existing == source)
            .unwrap_or(false)
        {
            return Ok(());
        }
        fs::remove_file(target)?;
    }
    symlink(source, target).with_context(|| {
        format!(
            "failed to symlink {} -> {}",
            target.display(),
            source.display()
        )
    })
}

#[cfg(not(unix))]
fn link_file(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::copy(source, target)?;
    Ok(())
}

fn item_slug(item: &crate::zotero::ItemSummary) -> String {
    let id = item
        .arxiv
        .as_ref()
        .or(item.doi.as_ref())
        .unwrap_or(&item.key);
    let title = item.title.as_deref().unwrap_or("untitled");
    let year = item.year.map(|year| format!("{year}-")).unwrap_or_default();
    safe_name(&format!("{year}{id}-{title}"))
        .chars()
        .take(120)
        .collect()
}

fn safe_name(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ') {
            output.push(ch);
        } else {
            output.push('-');
        }
    }
    let output = output.split_whitespace().collect::<Vec<_>>().join(" ");
    output.trim_matches(['.', '-', ' ']).to_string()
}
