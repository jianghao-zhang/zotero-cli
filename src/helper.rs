use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::{config::Config, paths};

pub const HELPER_ID: &str = "zcli-helper@zotero-cli.local";
pub const API_KEY_URL: &str = "https://www.zotero.org/settings/keys";
pub const API_LIBRARY_ID_HELP_URL: &str =
    "https://www.zotero.org/support/dev/web_api/v3/basics#user_and_group_library_urls";

#[derive(Debug)]
pub struct HelperInstallOptions {
    pub dry_run: bool,
    pub execute: bool,
    pub profile: Option<PathBuf>,
    pub force: bool,
}

#[derive(Debug)]
pub struct HelperPackageOptions {
    pub output: Option<PathBuf>,
    pub dry_run: bool,
    pub force: bool,
}

pub fn call(config: &Config, op: &str, params: Value) -> Result<Value> {
    let token_path =
        helper_token_path(config).ok_or_else(|| anyhow!("helper token path is not configured"))?;
    let token = fs::read_to_string(&token_path)
        .with_context(|| {
            format!(
                "helper token is missing at {}; install/start the Zotero helper plugin first",
                token_path.display()
            )
        })?
        .trim()
        .to_string();
    if token.is_empty() {
        return Err(anyhow!(
            "helper token file is empty: {}",
            token_path.display()
        ));
    }
    helper_post(
        &config.helper.endpoint,
        json!({
            "op": op,
            "token": token,
            "dry_run": false,
            "compact": true,
            "params": params,
        }),
    )
}

pub fn doctor(config: &Config) -> Result<Value> {
    let source = source_dir();
    let token_path = helper_token_path(config);
    let token = token_path
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty());
    let status_probe = helper_get(&config.helper.endpoint);
    let ping = token.as_deref().map(|token| {
        helper_post(
            &config.helper.endpoint,
            json!({"op": "ping", "token": token}),
        )
    });
    let (status, response, error, unauthenticated_response) = match (ping, status_probe) {
        (Some(Ok(value)), probe) => ("available", Some(value), None, probe.ok()),
        (Some(Err(err)), Ok(value)) => (
            "token_invalid_or_stale",
            None,
            Some(err.to_string()),
            Some(value),
        ),
        (Some(Err(err)), Err(probe_err)) => (
            "unavailable",
            None,
            Some(format!("{}; {}", err, probe_err)),
            None,
        ),
        (None, Ok(value)) => ("token_missing", None, None, Some(value)),
        (None, Err(err)) => (
            "not_installed_or_server_unreachable",
            None,
            Some(err.to_string()),
            None,
        ),
    };

    Ok(json!({
        "ok": true,
        "status": status,
        "optional": true,
        "source": source,
        "source_exists": source.join("manifest.json").exists() && source.join("bootstrap.js").exists(),
        "endpoint": config.helper.endpoint,
        "token_path": token_path,
        "token_present": token.is_some(),
        "installed_response": response,
        "unauthenticated_response": unauthenticated_response,
        "error": error,
        "capabilities": [
            "ping",
            "batch",
            "apply_tags",
            "move_to_collection",
            "create_note",
            "import_local_files",
            "link_attachment",
            "rename_attachment",
            "trash_items"
        ],
        "safety": {
            "core_cli_dependency": false,
            "arbitrary_js": false,
            "requires_token": true,
            "dry_run_first": true,
            "sqlite_writes": false
        },
        "performance": {
            "mode": "fast",
            "token_cached_in_plugin": true,
            "compact_execute_responses": true,
            "batch_supported": true,
            "rust_tcp_nodelay": true
        }
    }))
}

pub fn package(options: HelperPackageOptions, config: &Config) -> Result<Value> {
    let source = source_dir();
    let output = options
        .output
        .clone()
        .unwrap_or_else(|| default_package_path(config));
    let files = helper_files(&source)?;
    if options.dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "source": source,
            "output": output,
            "files": files,
            "requires": "zip command on PATH",
        }));
    }
    write_xpi(&source, &output, options.force)?;
    Ok(json!({
        "ok": true,
        "dry_run": false,
        "source": source,
        "output": output,
        "files": files,
        "install_note": "Install this XPI from Zotero's Add-ons window or run zcli helper install --execute.",
    }))
}

pub fn install(options: HelperInstallOptions, config: &Config) -> Result<Value> {
    if options.dry_run && options.execute {
        return Err(anyhow!("--dry-run and --execute cannot be used together"));
    }
    if !options.dry_run && !options.execute {
        return Err(anyhow!(
            "helper install is dry-run-first; pass --dry-run to preview or --execute to copy the XPI"
        ));
    }

    let profiles = detect_profiles();
    let selected_profile = select_profile(options.profile.clone(), &profiles)?;
    let package_path = default_package_path(config);
    let target_xpi = selected_profile
        .as_ref()
        .map(|profile| profile.join("extensions").join(format!("{HELPER_ID}.xpi")));
    let dry_run = options.dry_run;
    if dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "helper_id": HELPER_ID,
            "package_path": package_path,
            "detected_profiles": profiles,
            "selected_profile": selected_profile,
            "target_xpi": target_xpi,
            "will_write": false,
            "restart_required": true,
        }));
    }

    let Some(profile) = selected_profile else {
        return Err(anyhow!(
            "could not select a Zotero profile; pass --profile <path> or install the packaged XPI manually"
        ));
    };
    let extensions_dir = profile.join("extensions");
    fs::create_dir_all(&extensions_dir)?;
    write_xpi(&source_dir(), &package_path, true)?;
    let target = extensions_dir.join(format!("{HELPER_ID}.xpi"));
    if target.exists() && !options.force {
        return Ok(json!({
            "ok": true,
            "dry_run": false,
            "status": "already_exists",
            "profile": profile,
            "package_path": package_path,
            "target_xpi": target,
            "restart_required": true,
            "hint": "pass --force to overwrite the existing helper XPI",
        }));
    }
    fs::copy(&package_path, &target)?;
    Ok(json!({
        "ok": true,
        "dry_run": false,
        "status": "installed",
        "profile": profile,
        "package_path": package_path,
        "target_xpi": target,
        "restart_required": true,
    }))
}

fn source_dir() -> PathBuf {
    paths::package_root()
        .join("helper")
        .join("zcli-helper-zotero")
}

fn helper_token_path(config: &Config) -> Option<PathBuf> {
    config.helper.token_path.clone().or_else(|| {
        config
            .zotero_db_path
            .as_ref()
            .and_then(|path| path.parent().map(|parent| parent.join("zcli-helper-token")))
    })
}

fn default_package_path(config: &Config) -> PathBuf {
    config
        .cache_dir
        .clone()
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("zotero-cli")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zcli-helper-zotero.xpi")
}

fn helper_files(source: &Path) -> Result<Vec<PathBuf>> {
    if !source.exists() {
        return Err(anyhow!(
            "helper plugin source is missing: {}",
            source.display()
        ));
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry?;
        if entry.file_type().is_file() {
            files.push(entry.path().strip_prefix(source)?.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn write_xpi(source: &Path, output: &Path, force: bool) -> Result<()> {
    if !source.join("manifest.json").exists() || !source.join("bootstrap.js").exists() {
        return Err(anyhow!(
            "invalid helper plugin source: {}",
            source.display()
        ));
    }
    if output.exists() && !force {
        return Err(anyhow!(
            "helper package already exists: {}; pass --force to overwrite",
            output.display()
        ));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    if output.exists() {
        fs::remove_file(output)?;
    }
    let status = Command::new("zip")
        .arg("-qr")
        .arg(output)
        .arg(".")
        .current_dir(source)
        .status()
        .context("failed to run zip; install zip or use the helper source directory manually")?;
    if !status.success() {
        return Err(anyhow!("zip failed while packaging helper plugin"));
    }
    Ok(())
}

fn detect_profiles() -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return profiles;
    };
    let roots = [
        home.join("Library")
            .join("Application Support")
            .join("Zotero")
            .join("Profiles"),
        home.join(".zotero").join("zotero"),
    ];
    for root in roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    profiles.push(path);
                }
            }
        }
    }
    profiles.sort();
    profiles
}

fn select_profile(requested: Option<PathBuf>, profiles: &[PathBuf]) -> Result<Option<PathBuf>> {
    if let Some(path) = requested {
        return Ok(Some(path));
    }
    match profiles.len() {
        0 => Ok(None),
        1 => Ok(Some(profiles[0].clone())),
        _ => Err(anyhow!(
            "multiple Zotero profiles detected; pass --profile <path>"
        )),
    }
}

fn helper_post(endpoint: &str, payload: Value) -> Result<Value> {
    helper_request(endpoint, "POST", Some(payload))
}

fn helper_get(endpoint: &str) -> Result<Value> {
    helper_request(endpoint, "GET", None)
}

fn helper_request(endpoint: &str, method: &str, payload: Option<Value>) -> Result<Value> {
    let target = parse_http_endpoint(endpoint)?;
    let body = payload
        .map(|value| serde_json::to_string(&value))
        .transpose()?
        .unwrap_or_default();
    let mut stream = TcpStream::connect((&*target.host, target.port)).with_context(|| {
        format!(
            "could not connect to helper endpoint {}:{}",
            target.host, target.port
        )
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_nodelay(true)?;
    if method == "POST" {
        write!(
            stream,
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            target.path,
            target.host,
            target.port,
            body.len(),
            body
        )?;
    } else {
        write!(
            stream,
            "GET {} HTTP/1.1\r\nHost: {}:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            target.path,
            target.host,
            target.port
        )?;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    parse_helper_response(&response)
}

fn parse_helper_response(response: &str) -> Result<Value> {
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("invalid helper HTTP response"))?;
    let status = http_status(head)?;
    let parsed: Value = serde_json::from_str(body)
        .with_context(|| format!("invalid helper JSON response: {}", body.trim()))?;
    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "helper endpoint returned HTTP {status}: {}",
            helper_error_message(&parsed).unwrap_or_else(|| body.trim().to_string())
        ));
    }
    if parsed.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(anyhow!(
            "helper endpoint returned error: {}",
            helper_error_message(&parsed).unwrap_or_else(|| parsed.to_string())
        ));
    }
    Ok(parsed)
}

fn http_status(head: &str) -> Result<u16> {
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| anyhow!("missing helper HTTP status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("invalid helper HTTP status line: {status_line}"))?
        .parse::<u16>()?;
    Ok(status)
}

fn helper_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|message| !message.is_empty())
}

struct HttpTarget {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_endpoint(endpoint: &str) -> Result<HttpTarget> {
    let raw = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("helper endpoint must use http://"))?;
    let (authority, path) = raw.split_once('/').unwrap_or((raw, ""));
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_string(), port.parse::<u16>()?)
    } else {
        (authority.to_string(), 80)
    };
    if host.is_empty() {
        return Err(anyhow!("helper endpoint host is empty"));
    }
    Ok(HttpTarget {
        host,
        port,
        path: format!("/{}", path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_helper_response_rejects_non_2xx_status() {
        let error = parse_helper_response(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: 36\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"bad token\"}",
        )
        .expect_err("non-2xx helper responses must fail");
        assert!(error.to_string().contains("HTTP 500"));
        assert!(error.to_string().contains("bad token"));
    }

    #[test]
    fn parse_helper_response_rejects_ok_false_on_2xx_status() {
        let error = parse_helper_response(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 39\r\nConnection: close\r\n\r\n{\"ok\":false,\"error\":\"missing item\"}",
        )
        .expect_err("ok:false helper responses must fail");
        assert!(error.to_string().contains("missing item"));
    }

    #[test]
    fn parse_helper_response_accepts_success_status() {
        let value = parse_helper_response(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 21\r\nConnection: close\r\n\r\n{\"ok\":true,\"pong\":1}",
        )
        .unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["pong"], 1);
    }
}
