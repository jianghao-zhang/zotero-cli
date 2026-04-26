use std::{
    env, fs,
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::{
    cli::{Context, SetupArgs},
    config::Config,
    helper::{API_KEY_URL, API_LIBRARY_ID_HELP_URL},
    skill::{self, SkillInstallOptions, SkillTarget},
};

pub fn run(context: &Context, args: &SetupArgs) -> Result<Value> {
    let mut wizard = SetupWizard::new(context, args);
    if args.defaults {
        wizard.apply_defaults();
    } else {
        wizard.prompt_all()?;
    }
    wizard.finish()
}

struct SetupWizard<'a> {
    context: &'a Context,
    args: &'a SetupArgs,
    config: Config,
    overwrite_existing: bool,
    skill_targets: Vec<SkillTarget>,
    notes: Vec<String>,
}

impl<'a> SetupWizard<'a> {
    fn new(context: &'a Context, args: &'a SetupArgs) -> Self {
        Self {
            context,
            args,
            config: context.config.clone(),
            overwrite_existing: args.force,
            skill_targets: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn apply_defaults(&mut self) {
        if self.config.web_api.api_key_env.is_none() {
            self.config.web_api.api_key_env = Some("ZOTERO_API_KEY".to_string());
        }
        if self.config.lfz.enabled.is_none() {
            self.config.lfz.enabled = Some(
                self.config
                    .lfz
                    .claude_runtime_dir
                    .as_deref()
                    .map(|path| path.exists())
                    .unwrap_or(false),
            );
        }
        self.notes
            .push("used detected/default values without interactive prompts".to_string());
    }

    fn prompt_all(&mut self) -> Result<()> {
        write_title("zcli setup")?;
        write_line("This writes local zcli config only. It does not contact Zotero Web API, import papers, or mutate your Zotero library.")?;
        self.prompt_existing_config()?;
        write_line("Press Enter to accept a detected/default value.")?;

        self.prompt_zotero_paths()?;
        self.prompt_mirror()?;
        self.prompt_web_api()?;
        self.prompt_lfz()?;
        if !self.args.no_skills {
            self.prompt_skills()?;
        }
        Ok(())
    }

    fn prompt_existing_config(&mut self) -> Result<()> {
        if self.args.dry_run || self.args.force || !self.context.config_path.exists() {
            return Ok(());
        }
        write_line(&format!(
            "Existing config found: {}",
            self.context.config_path.display()
        ))?;
        write_line(
            "This setup run will update that local config file only if you approve overwrite here.",
        )?;
        let overwrite = prompt_bool("Overwrite existing config when setup finishes?", false)?;
        if !overwrite {
            return Err(anyhow!(
                "config already exists: {}; rerun with --force or answer yes to overwrite",
                self.context.config_path.display()
            ));
        }
        self.overwrite_existing = true;
        self.notes
            .push("user approved overwriting existing config".to_string());
        Ok(())
    }

    fn prompt_zotero_paths(&mut self) -> Result<()> {
        write_section("Local Zotero library")?;
        write_line("These two paths are auto-detected on normal Zotero installs. zcli can also auto-detect them at runtime, but saving them makes behavior explicit.")?;
        write_line("The database stores item metadata, collections, tags, notes, annotations, attachment indexes, and optional llm-for-zotero tables.")?;
        let db_default = self.config.zotero_db_path.clone();
        let db = prompt_path("Zotero database path", db_default)?;
        self.config.zotero_db_path = db;

        write_line("The storage folder contains PDFs, attachment files, and Zotero full-text cache files used by extract/search/mirror.")?;
        let storage_default = self.config.zotero_storage_path.clone().or_else(|| {
            self.config
                .zotero_db_path
                .as_ref()
                .and_then(|db| db.parent().map(|parent| parent.join("storage")))
        });
        self.config.zotero_storage_path = prompt_path("Zotero storage path", storage_default)?;
        Ok(())
    }

    fn prompt_mirror(&mut self) -> Result<()> {
        write_section("Filesystem mirror")?;
        write_line("Optional. A mirror is a generated folder view of your Zotero library for file-native workflows and external agents.")?;
        write_line("It is maintained by `zcli mirror sync` or the foreground watcher `zcli mirror watch`; it is not a live Zotero feature by itself.")?;
        write_line("Skip this unless you want collection folders, an Allin/ flat index, metadata.json/paper.md files, and attachment symlinks/copies outside Zotero.")?;
        let configure = prompt_bool(
            "Configure optional filesystem mirror root?",
            self.config.mirror_root.is_some(),
        )?;
        if configure {
            let default = self
                .config
                .mirror_root
                .clone()
                .or_else(|| dirs::home_dir().map(|home| home.join("ZoteroMirror")));
            self.config.mirror_root = prompt_path("Mirror root", default)?;
        } else {
            self.config.mirror_root = None;
        }
        Ok(())
    }

    fn prompt_web_api(&mut self) -> Result<()> {
        write_section("Zotero Web API")?;
        write_line("Optional. Core zcli commands are local and do not need the Web API.")?;
        write_line("Configure this only if you want to save a Zotero online library identity/API key for future sync, remote read, or import workflows.")?;
        write_line(&format!(
            "Create or manage Zotero API keys here: {API_KEY_URL}"
        ))?;
        write_line("Library ID means Zotero's numeric API id, not your username, email, library name, or local SQLite libraryID.")?;
        write_line("For a personal library, use library type `user` and the `Your userID for use in API calls` number shown on the API Keys page.")?;
        write_line("For a group library, use library type `group` and the numeric groupID from the group URL/settings link, or retrieve group IDs from `/users/<userID>/groups`.")?;
        write_line(&format!(
            "Official library ID docs: {API_LIBRARY_ID_HELP_URL}"
        ))?;
        write_line("Using an env var such as ZOTERO_API_KEY is preferred; stored keys are redacted in output but still live in config.toml.")?;
        let configure = prompt_bool(
            "Configure optional Zotero Web API?",
            self.config.web_api.enabled,
        )?;
        self.config.web_api.enabled = configure;
        if !configure {
            return Ok(());
        }

        let library_type = prompt_string(
            "Library type [user/group]",
            Some(self.config.web_api.library_type.as_str()),
        )?;
        let library_type = match library_type.trim().to_ascii_lowercase().as_str() {
            "group" | "groups" => "group",
            _ => "user",
        };
        self.config.web_api.library_type = library_type.to_string();

        let id_label = if library_type == "group" {
            "Library ID (numeric groupID)"
        } else {
            "Library ID (numeric userID)"
        };
        let library_id = prompt_string(id_label, self.config.web_api.library_id.as_deref())?;
        self.config.web_api.library_id = empty_to_none(library_id);

        let base_url = prompt_string("Base URL", Some(self.config.web_api.base_url.as_str()))?;
        if !base_url.trim().is_empty() {
            self.config.web_api.base_url = base_url.trim().trim_end_matches('/').to_string();
        }

        let key_mode = prompt_string(
            "API key source [env/stored/skip]",
            Some(if self.config.web_api.api_key.is_some() {
                "stored"
            } else {
                "env"
            }),
        )?;
        match key_mode.trim().to_ascii_lowercase().as_str() {
            "stored" | "store" => {
                write_line(
                    "Stored key input is visible in the terminal; prefer env for shared machines.",
                )?;
                let key = prompt_string("Paste API key", self.config.web_api.api_key.as_deref())?;
                self.config.web_api.api_key = empty_to_none(key);
            }
            "skip" | "none" => {
                self.config.web_api.api_key = None;
            }
            _ => {
                let env_name = prompt_string(
                    "API key env var",
                    self.config
                        .web_api
                        .api_key_env
                        .as_deref()
                        .or(Some("ZOTERO_API_KEY")),
                )?;
                self.config.web_api.api_key_env = Some(if env_name.trim().is_empty() {
                    "ZOTERO_API_KEY".to_string()
                } else {
                    env_name.trim().to_string()
                });
                self.config.web_api.api_key = None;
            }
        }
        Ok(())
    }

    fn prompt_lfz(&mut self) -> Result<()> {
        write_section("llm-for-zotero integration")?;
        write_line("Optional. Enable this if you use llm-for-zotero and want `zcli recap reading` and `zcli recap lfz` to include LLM chats, Claude Code runtime metadata, final answers, and event counts.")?;
        write_line("Skip it if you only want normal Zotero CLI/search/reading recap behavior.")?;
        let runtime_exists = self
            .config
            .lfz
            .claude_runtime_dir
            .as_deref()
            .map(|path| path.exists())
            .unwrap_or(false);
        let default = self.config.lfz.enabled.unwrap_or(runtime_exists);
        let configure = prompt_bool("Configure optional llm-for-zotero support?", default)?;
        self.config.lfz.enabled = Some(configure);
        if configure {
            let default_dir =
                self.config.lfz.claude_runtime_dir.clone().or_else(|| {
                    dirs::home_dir().map(|home| home.join("Zotero").join("agent-runtime"))
                });
            self.config.lfz.claude_runtime_dir =
                prompt_path("llm-for-zotero Claude runtime dir", default_dir)?;
        }
        Ok(())
    }

    fn prompt_skills(&mut self) -> Result<()> {
        write_section("Agent skill")?;
        write_line("Optional. This installs a small SKILL.md so Codex, Claude Code, Hermes, llm-for-zotero runtime, or OpenClaw know to call `zcli` directly.")?;
        write_line("It is not required for humans using the CLI. You can install later with `zcli skill install --target <agent> --dry-run`.")?;
        let install = prompt_bool("Install optional agent skill now?", false)?;
        if !install {
            return Ok(());
        }
        let raw = prompt_string(
            "Skill targets comma-separated [codex,claude,hermes,lfz,openclaw]",
            Some("codex,claude"),
        )?;
        let mut targets = Vec::new();
        for part in raw.split(',') {
            let value = part.trim().to_ascii_lowercase();
            let target = match value.as_str() {
                "codex" => Some(SkillTarget::Codex),
                "claude" | "claude-code" | "claude_code" => Some(SkillTarget::Claude),
                "hermes" => Some(SkillTarget::Hermes),
                "lfz" | "llm-for-zotero" => Some(SkillTarget::Lfz),
                "openclaw" => Some(SkillTarget::Openclaw),
                "" => None,
                other => return Err(anyhow!("unknown skill target: {other}")),
            };
            if let Some(target) = target {
                targets.push(target);
            }
        }
        self.skill_targets = targets;
        Ok(())
    }

    fn finish(self) -> Result<Value> {
        let config_exists = self.context.config_path.exists();
        if config_exists && !self.args.force && !self.args.dry_run && !self.overwrite_existing {
            return Err(anyhow!(
                "config already exists: {}; pass --force to overwrite",
                self.context.config_path.display()
            ));
        }

        let mut skill_results = Vec::new();
        for target in self.skill_targets {
            skill_results.push(skill::install(SkillInstallOptions {
                target,
                dry_run: self.args.dry_run,
                copy: false,
            })?);
        }

        if !self.args.dry_run {
            if let Some(parent) = self.context.config_path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.config.save(&self.context.config_path, true)?;
        }

        Ok(json!({
            "ok": true,
            "dry_run": self.args.dry_run,
            "wrote_config": !self.args.dry_run,
            "config_path": self.context.config_path,
            "config": redacted_config(&self.config),
            "skill_installs": skill_results,
            "notes": self.notes,
        }))
    }
}

fn prompt_path(label: &str, default: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let rendered = default
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let value = prompt_string(
        label,
        if rendered.is_empty() {
            None
        } else {
            Some(&rendered)
        },
    )?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(default);
    }
    Ok(Some(expand_tilde(trimmed)))
}

fn prompt_bool(label: &str, default: bool) -> Result<bool> {
    let default_label = if default { "Y/n" } else { "y/N" };
    loop {
        let answer = prompt_string(&format!("{label} [{default_label}]"), None)?;
        let normalized = answer.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => write_line("Please answer yes or no.")?,
        }
    }
}

fn prompt_string(label: &str, default: Option<&str>) -> Result<String> {
    let mut stderr = io::stderr();
    if is_human_terminal() {
        let width = terminal_width();
        match default {
            Some(default) if !default.is_empty() => {
                let inline_len = label.chars().count() + default.chars().count() + 5;
                if inline_len > width.saturating_sub(2) {
                    writeln!(
                        stderr,
                        "{} {}",
                        style_text("?", AnsiStyle::Accent),
                        style_text(label, AnsiStyle::Prompt)
                    )?;
                    write_wrapped(&format!("default: {default}"), AnsiStyle::Muted, 2)?;
                    write!(stderr, "{} ", style_text(">", AnsiStyle::Accent))?;
                } else {
                    write!(
                        stderr,
                        "{} {} {} ",
                        style_text("?", AnsiStyle::Accent),
                        style_text(label, AnsiStyle::Prompt),
                        style_text(&format!("[{default}]:"), AnsiStyle::Muted)
                    )?;
                }
            }
            _ => write!(
                stderr,
                "{} {} ",
                style_text("?", AnsiStyle::Accent),
                style_text(&format!("{label}:"), AnsiStyle::Prompt)
            )?,
        }
    } else {
        match default {
            Some(default) if !default.is_empty() => write!(stderr, "{label} [{default}]: ")?,
            _ => write!(stderr, "{label}: ")?,
        }
    }
    stderr.flush()?;
    let mut value = String::new();
    let read = io::stdin().read_line(&mut value)?;
    if read == 0 {
        return Ok(default.unwrap_or_default().to_string());
    }
    let value = value.trim_end_matches(['\n', '\r']);
    if value.trim().is_empty() {
        Ok(default.unwrap_or_default().to_string())
    } else {
        Ok(value.to_string())
    }
}

#[derive(Clone, Copy)]
enum AnsiStyle {
    Accent,
    Heading,
    Muted,
    Prompt,
}

fn write_title(message: &str) -> Result<()> {
    writeln!(io::stderr(), "{}", style_text(message, AnsiStyle::Heading))?;
    Ok(())
}

fn write_line(message: &str) -> Result<()> {
    write_wrapped(message, AnsiStyle::Muted, 2)
}

fn write_wrapped(message: &str, style: AnsiStyle, indent: usize) -> Result<()> {
    let width = terminal_width();
    let indent_text = " ".repeat(indent);
    for line in wrap_text(message, width.saturating_sub(indent).max(40)) {
        writeln!(io::stderr(), "{indent_text}{}", style_text(&line, style))?;
    }
    Ok(())
}

fn write_section(title: &str) -> Result<()> {
    writeln!(io::stderr())?;
    writeln!(io::stderr(), "{}", style_text(title, AnsiStyle::Heading))?;
    Ok(())
}

fn wrap_text(message: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in message.lines() {
        let mut line = String::new();
        for word in raw.split_whitespace() {
            let word_len = word.chars().count();
            let line_len = line.chars().count();
            if line_len == 0 {
                line.push_str(word);
            } else if line_len + 1 + word_len <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(line);
                line = word.to_string();
            }
        }
        if line.is_empty() {
            lines.push(String::new());
        } else {
            lines.push(line);
        }
    }
    lines
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| (60..=160).contains(width))
        .unwrap_or(88)
}

fn style_text(message: &str, style: AnsiStyle) -> String {
    if !should_color() {
        return message.to_string();
    }
    let code = match style {
        AnsiStyle::Accent => "36",
        AnsiStyle::Heading => "1;36",
        AnsiStyle::Muted => "2",
        AnsiStyle::Prompt => "1",
    };
    format!("\x1b[{code}m{message}\x1b[0m")
}

fn should_color() -> bool {
    is_human_terminal()
        && env::var_os("NO_COLOR").is_none()
        && env::var("CLICOLOR")
            .map(|value| value != "0")
            .unwrap_or(true)
        && env::var("TERM")
            .map(|value| value != "dumb")
            .unwrap_or(true)
}

fn is_human_terminal() -> bool {
    io::stderr().is_terminal()
}

fn empty_to_none(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn expand_tilde(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(value));
    }
    PathBuf::from(value)
}

fn redacted_config(config: &Config) -> Value {
    json!({
        "zotero_db_path": config.zotero_db_path,
        "zotero_storage_path": config.zotero_storage_path,
        "mirror_root": config.mirror_root,
        "cache_dir": config.cache_dir,
        "state_dir": config.state_dir,
        "web_api": {
            "enabled": config.web_api.enabled,
            "base_url": config.web_api.base_url,
            "library_type": config.web_api.library_type,
            "library_id": config.web_api.library_id,
            "library_id_help_url": API_LIBRARY_ID_HELP_URL,
            "api_key_url": API_KEY_URL,
            "api_key_env": config.web_api.api_key_env,
            "stored_api_key": config.web_api.api_key.as_ref().map(|_| "<redacted>"),
        },
        "helper": {
            "enabled": config.helper.enabled,
            "endpoint": config.helper.endpoint,
            "token_path": config.helper.token_path,
        },
        "lfz": config.lfz,
    })
}
