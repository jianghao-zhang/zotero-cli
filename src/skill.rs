use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use serde_json::{json, Value};

use crate::{config::Config, paths};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SkillTarget {
    Codex,
    Claude,
    Hermes,
    Lfz,
    Openclaw,
}

#[derive(Clone, Copy, Debug)]
pub struct SkillInstallOptions {
    pub target: SkillTarget,
    pub dry_run: bool,
    pub copy: bool,
}

#[derive(Clone, Debug)]
struct SkillDestination {
    path: PathBuf,
    kind: &'static str,
    root: PathBuf,
}

pub fn install(options: SkillInstallOptions, config: &Config) -> Result<Value> {
    let source = source_skill_dir(options.target);
    let destinations = target_destinations(options.target, config);
    let availability = target_availability(options.target, &destinations);
    let operation = if options.copy { "copy" } else { "symlink" };
    let primary_target = destinations
        .first()
        .map(|destination| destination.path.clone())
        .unwrap_or_else(|| fallback_target_path(options.target, config));

    if options.dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "target": target_name(options.target),
            "available": availability.available,
            "reason": availability.reason,
            "operation": operation,
            "source": source,
            "source_exists": source.join("SKILL.md").exists(),
            "target_path": primary_target,
            "target_paths": destinations_json(&destinations),
        }));
    }

    if !source.join("SKILL.md").exists() {
        return Err(anyhow!(
            "zotero-cli skill source is missing: {}",
            source.display()
        ));
    }
    if !availability.available {
        return Err(anyhow!(
            "target {} is unavailable: {}",
            target_name(options.target),
            availability.reason.unwrap_or("unknown".to_string())
        ));
    }

    let mut installs = Vec::new();
    for destination in &destinations {
        if destination.path.exists() {
            installs.push(json!({
                "status": "already_exists",
                "kind": destination.kind,
                "root": destination.root,
                "target_path": destination.path,
            }));
            continue;
        }
        if let Some(parent) = destination.path.parent() {
            fs::create_dir_all(parent)?;
        }
        if options.copy {
            copy_dir_all(&source, &destination.path)?;
        } else {
            symlink_or_copy(&source, &destination.path)?;
        }
        installs.push(json!({
            "status": "installed",
            "kind": destination.kind,
            "root": destination.root,
            "target_path": destination.path,
        }));
    }

    let status = if installs
        .iter()
        .all(|install| install.get("status").and_then(Value::as_str) == Some("already_exists"))
    {
        "already_exists"
    } else {
        "installed"
    };

    Ok(json!({
        "ok": true,
        "dry_run": false,
        "target": target_name(options.target),
        "status": status,
        "operation": operation,
        "source": source,
        "target_path": primary_target,
        "target_paths": destinations_json(&destinations),
        "installs": installs,
    }))
}

pub fn doctor(config: &Config) -> Result<Value> {
    let targets = [
        SkillTarget::Codex,
        SkillTarget::Claude,
        SkillTarget::Hermes,
        SkillTarget::Lfz,
        SkillTarget::Openclaw,
    ];
    Ok(json!({
        "ok": true,
        "source": source_skill_dir(SkillTarget::Codex),
        "source_exists": source_skill_dir(SkillTarget::Codex).join("SKILL.md").exists(),
        "targets": targets.iter().map(|target| {
            let source = source_skill_dir(*target);
            let destinations = target_destinations(*target, config);
            let availability = target_availability(*target, &destinations);
            let primary = destinations
                .first()
                .map(|destination| destination.path.clone())
                .unwrap_or_else(|| fallback_target_path(*target, config));
            json!({
                "target": target_name(*target),
                "source": source,
                "source_exists": source.join("SKILL.md").exists(),
                "path": primary,
                "target_paths": destinations_json(&destinations),
                "installed": destinations.iter().any(|destination| destination.path.exists()),
                "available": availability.available,
                "reason": availability.reason,
                "is_symlink": fs::symlink_metadata(&primary).map(|m| m.file_type().is_symlink()).unwrap_or(false),
            })
        }).collect::<Vec<_>>(),
    }))
}

fn source_skill_dir(target: SkillTarget) -> PathBuf {
    let root = paths::package_root().join("skills");
    match target {
        SkillTarget::Lfz => root.join("zotero-cli-lfz"),
        _ => root.join("zotero-cli"),
    }
}

fn fallback_target_path(target: SkillTarget, config: &Config) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    match target {
        SkillTarget::Codex => home.join(".codex").join("skills").join("zotero-cli"),
        SkillTarget::Claude => home.join(".claude").join("skills").join("zotero-cli"),
        SkillTarget::Hermes => home.join(".hermes").join("skills").join("zotero-cli"),
        SkillTarget::Lfz => config
            .lfz
            .claude_runtime_dir
            .clone()
            .unwrap_or_else(|| home.join("Zotero").join("agent-runtime"))
            .join(".claude")
            .join("skills")
            .join("zotero-cli"),
        SkillTarget::Openclaw => home.join(".openclaw").join("skills").join("zotero-cli"),
    }
}

fn target_destinations(target: SkillTarget, config: &Config) -> Vec<SkillDestination> {
    match target {
        SkillTarget::Lfz => lfz_runtime_destinations(config),
        _ => vec![SkillDestination {
            path: fallback_target_path(target, config),
            kind: target_name(target),
            root: fallback_target_path(target, config)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        }],
    }
}

fn lfz_runtime_destinations(config: &Config) -> Vec<SkillDestination> {
    let mut seen = HashSet::new();
    let mut destinations = Vec::new();

    if let Some(runtime_dir) = config.lfz.claude_runtime_dir.as_ref() {
        add_lfz_profile_runtime_candidates(
            runtime_dir,
            "configured_lfz_runtime",
            &mut destinations,
            &mut seen,
        );
    }
    if let Some(zotero_data_dir) = config.lfz.zotero_data_dir.as_ref() {
        add_lfz_profile_runtime_candidates(
            &zotero_data_dir.join("agent-runtime"),
            "zotero_data_runtime",
            &mut destinations,
            &mut seen,
        );
    }
    if let Some(home) = dirs::home_dir() {
        add_lfz_profile_runtime_candidates(
            &home.join("Zotero").join("agent-runtime"),
            "home_zotero_runtime",
            &mut destinations,
            &mut seen,
        );
    }

    destinations
}

fn add_lfz_profile_runtime_candidates(
    runtime: &Path,
    kind: &'static str,
    destinations: &mut Vec<SkillDestination>,
    seen: &mut HashSet<PathBuf>,
) {
    if !runtime.exists() {
        return;
    }

    if let Some(profile_root) = lfz_profile_runtime_root_from_path(runtime) {
        push_destination(&profile_root, kind, destinations, seen);
        return;
    }

    let Some(name) = runtime.file_name().and_then(|name| name.to_str()) else {
        return;
    };
    if name != "agent-runtime" {
        return;
    };

    let Ok(entries) = fs::read_dir(runtime) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() && is_lfz_profile_runtime_root(&entry.path()) {
            push_destination(&entry.path(), "zotero_profile_runtime", destinations, seen);
        }
    }
}

fn lfz_profile_runtime_root_from_path(path: &Path) -> Option<PathBuf> {
    if is_lfz_profile_runtime_root(path) {
        return Some(path.to_path_buf());
    }
    match path.file_name().and_then(|name| name.to_str()) {
        Some(".claude") => path
            .parent()
            .filter(|parent| is_lfz_profile_runtime_root(parent))
            .map(Path::to_path_buf),
        Some("skills") => path
            .parent()
            .and_then(Path::parent)
            .filter(|parent| is_lfz_profile_runtime_root(parent))
            .map(Path::to_path_buf),
        Some("zotero-cli") => path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .filter(|parent| is_lfz_profile_runtime_root(parent))
            .map(Path::to_path_buf),
        _ => None,
    }
}

fn is_lfz_profile_runtime_root(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("profile-"))
        .unwrap_or(false)
        && path.join(".claude").exists()
}

fn push_destination(
    runtime: &Path,
    kind: &'static str,
    destinations: &mut Vec<SkillDestination>,
    seen: &mut HashSet<PathBuf>,
) {
    let path = lfz_skill_target_from_runtime_path(runtime);
    if seen.insert(path.clone()) {
        destinations.push(SkillDestination {
            path,
            kind,
            root: runtime.to_path_buf(),
        });
    }
}

fn lfz_skill_target_from_runtime_path(path: &Path) -> PathBuf {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("zotero-cli") => path.to_path_buf(),
        Some("skills") => path.join("zotero-cli"),
        Some(".claude") => path.join("skills").join("zotero-cli"),
        _ => path.join(".claude").join("skills").join("zotero-cli"),
    }
}

fn destinations_json(destinations: &[SkillDestination]) -> Vec<Value> {
    destinations
        .iter()
        .map(|destination| {
            json!({
                "kind": destination.kind,
                "root": destination.root,
                "path": destination.path,
                "installed": destination.path.exists(),
                "is_symlink": fs::symlink_metadata(&destination.path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false),
            })
        })
        .collect()
}

struct Availability {
    available: bool,
    reason: Option<String>,
}

fn target_availability(target: SkillTarget, destinations: &[SkillDestination]) -> Availability {
    match target {
        SkillTarget::Lfz => {
            if destinations.is_empty() {
                return Availability {
                    available: false,
                    reason: Some(
                        "no llm-for-zotero Claude runtime roots were detected".to_string(),
                    ),
                };
            }
            Availability {
                available: true,
                reason: None,
            }
        }
        SkillTarget::Openclaw => {
            let Some(destination) = destinations.first() else {
                return Availability {
                    available: false,
                    reason: Some("OpenClaw skill target could not be resolved".to_string()),
                };
            };
            let root = destination.path.parent().unwrap_or_else(|| Path::new(""));
            if !root.exists() {
                return Availability {
                    available: false,
                    reason: Some(format!(
                        "OpenClaw skill root does not exist: {}",
                        root.display()
                    )),
                };
            }
            if !command_exists("openclaw") {
                return Availability {
                    available: false,
                    reason: Some("openclaw command was not found on PATH".to_string()),
                };
            }
            Availability {
                available: true,
                reason: None,
            }
        }
        _ => Availability {
            available: true,
            reason: None,
        },
    }
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(command);
                candidate.exists() && candidate.is_file()
            })
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn symlink_or_copy(source: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;
    symlink(source, target).with_context(|| {
        format!(
            "failed to symlink {} -> {}",
            target.display(),
            source.display()
        )
    })
}

#[cfg(not(unix))]
fn symlink_or_copy(source: &Path, target: &Path) -> Result<()> {
    copy_dir_all(source, target)
}

fn copy_dir_all(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let destination = target.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &destination)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn target_name(target: SkillTarget) -> &'static str {
    match target {
        SkillTarget::Codex => "codex",
        SkillTarget::Claude => "claude",
        SkillTarget::Hermes => "hermes",
        SkillTarget::Lfz => "lfz",
        SkillTarget::Openclaw => "openclaw",
    }
}
