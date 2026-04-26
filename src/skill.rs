use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use serde_json::{json, Value};

use crate::paths;

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

pub fn install(options: SkillInstallOptions) -> Result<Value> {
    let source = source_skill_dir();
    let target = target_path(options.target);
    let availability = target_availability(options.target, &target);
    let operation = if options.copy { "copy" } else { "symlink" };

    if options.dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "target": target_name(options.target),
            "available": availability.available,
            "reason": availability.reason,
            "operation": operation,
            "source": source,
            "target_path": target,
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
    if target.exists() {
        return Ok(json!({
            "ok": true,
            "dry_run": false,
            "target": target_name(options.target),
            "status": "already_exists",
            "source": source,
            "target_path": target,
        }));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    if options.copy {
        copy_dir_all(&source, &target)?;
    } else {
        symlink_or_copy(&source, &target)?;
    }

    Ok(json!({
        "ok": true,
        "dry_run": false,
        "target": target_name(options.target),
        "status": "installed",
        "operation": operation,
        "source": source,
        "target_path": target,
    }))
}

pub fn doctor() -> Result<Value> {
    let source = source_skill_dir();
    let targets = [
        SkillTarget::Codex,
        SkillTarget::Claude,
        SkillTarget::Hermes,
        SkillTarget::Lfz,
        SkillTarget::Openclaw,
    ];
    Ok(json!({
        "ok": true,
        "source": source,
        "source_exists": source.join("SKILL.md").exists(),
        "targets": targets.iter().map(|target| {
            let path = target_path(*target);
            let availability = target_availability(*target, &path);
            json!({
                "target": target_name(*target),
                "path": path,
                "installed": path.exists(),
                "available": availability.available,
                "reason": availability.reason,
                "is_symlink": fs::symlink_metadata(&path).map(|m| m.file_type().is_symlink()).unwrap_or(false),
            })
        }).collect::<Vec<_>>(),
    }))
}

fn source_skill_dir() -> PathBuf {
    paths::package_root().join("skills").join("zotero-cli")
}

fn target_path(target: SkillTarget) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    match target {
        SkillTarget::Codex => home.join(".codex").join("skills").join("zotero-cli"),
        SkillTarget::Claude => home.join(".claude").join("skills").join("zotero-cli"),
        SkillTarget::Hermes => home.join(".hermes").join("skills").join("zotero-cli"),
        SkillTarget::Lfz => home
            .join("Zotero")
            .join("agent-runtime")
            .join(".claude")
            .join("skills")
            .join("zotero-cli"),
        SkillTarget::Openclaw => home.join(".openclaw").join("skills").join("zotero-cli"),
    }
}

struct Availability {
    available: bool,
    reason: Option<String>,
}

fn target_availability(target: SkillTarget, target_path: &Path) -> Availability {
    match target {
        SkillTarget::Openclaw => {
            let root = target_path.parent().unwrap_or_else(|| Path::new(""));
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
