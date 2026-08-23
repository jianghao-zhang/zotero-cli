use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ureq::{http::Response, Agent, Body};

use crate::config::Config;

const APP_NAME: &str = "zotero-cli";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalApiKeyRecord {
    key: String,
    server_id: String,
    remember: bool,
}

#[derive(Debug)]
struct ApiResponse {
    status: u16,
    body: String,
    server_id: Option<String>,
    zotero_version: Option<String>,
    api_version: Option<String>,
    schema_version: Option<String>,
}

#[derive(Debug)]
struct WriteContext {
    record: LocalApiKeyRecord,
    key_path: PathBuf,
}

pub fn doctor(config: &Config) -> Result<Value> {
    let key_path = key_path(config);
    if !config.local_api.enabled {
        return Ok(json!({
            "ok": true,
            "status": "disabled",
            "enabled": false,
            "endpoint": config.local_api.endpoint,
            "key_path": key_path,
            "key_present": false,
        }));
    }

    let stored = load_key_record(key_path.as_deref()).ok().flatten();
    match probe(config) {
        Ok(response) => {
            let server_matches = stored.as_ref().and_then(|record| {
                response
                    .server_id
                    .as_ref()
                    .map(|id| id == &record.server_id)
            });
            let status = match (stored.is_some(), server_matches) {
                (true, Some(true)) => "available_authorized",
                (true, Some(false)) => "server_changed",
                _ => "authorization_required",
            };
            Ok(json!({
                "ok": true,
                "status": status,
                "enabled": true,
                "endpoint": config.local_api.endpoint,
                "zotero_running": true,
                "zotero_version": response.zotero_version,
                "api_version": response.api_version,
                "schema_version": response.schema_version,
                "server_id_present": response.server_id.is_some(),
                "server_matches_key": server_matches,
                "key_path": key_path,
                "key_present": stored.is_some(),
                "key_persistent": stored.as_ref().map(|record| record.remember),
                "implemented_operations": ["apply_tags", "move_to_collection", "create_note"],
                "zotero_10_capabilities": [
                    "items", "collections", "saved_searches", "tag_delete",
                    "file_upload", "fulltext_write"
                ],
            }))
        }
        Err(error) => Ok(json!({
            "ok": true,
            "status": "unavailable",
            "enabled": true,
            "endpoint": config.local_api.endpoint,
            "zotero_running": false,
            "key_path": key_path,
            "key_present": stored.is_some(),
            "error": error.to_string(),
            "implemented_operations": ["apply_tags", "move_to_collection", "create_note"],
            "zotero_10_capabilities": [
                "items", "collections", "saved_searches", "tag_delete",
                "file_upload", "fulltext_write"
            ],
        })),
    }
}

pub fn authorize(config: &Config, dry_run: bool, execute: bool) -> Result<Value> {
    if dry_run && execute {
        return Err(anyhow!("--dry-run and --execute cannot be used together"));
    }
    if !dry_run && !execute {
        return Err(anyhow!(
            "local API authorization is dry-run-first; pass --dry-run to preview or --execute to request access"
        ));
    }
    let key_path =
        key_path(config).ok_or_else(|| anyhow!("local API key path is not configured"))?;
    if dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "endpoint": config.local_api.endpoint,
            "key_path": key_path,
            "will_prompt_in_zotero": true,
            "recommended_choice": "Always Allow",
        }));
    }
    if !config.local_api.enabled {
        return Err(anyhow!("Zotero Local API is disabled in zcli config"));
    }

    let probe = probe(config)?;
    let server_id = probe
        .server_id
        .ok_or_else(|| anyhow!("Zotero Local API did not return Zotero-Server-ID"))?;
    let response = send_json(
        config,
        "POST",
        "local/authorize",
        &json!({"appName": APP_NAME}),
        Some(&server_id),
        None,
        false,
        // Authorization is interactive and may remain open while the user is
        // away from Zotero. Keep the request alive long enough for a deliberate
        // choice instead of turning a successful click into an orphaned key.
        Duration::from_secs(15 * 60),
    )?;
    ensure_success(&response, "authorize Zotero Local API")?;
    let value: Value = serde_json::from_str(&response.body)
        .context("Zotero Local API authorization returned invalid JSON")?;
    let key = value
        .get("key")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| anyhow!("Zotero Local API authorization returned no key"))?;
    let remember = value
        .get("remember")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    save_key_record(
        &key_path,
        &LocalApiKeyRecord {
            key: key.to_string(),
            server_id,
            remember,
        },
    )?;
    Ok(json!({
        "ok": true,
        "dry_run": false,
        "status": "authorized",
        "remember": remember,
        "key_path": key_path,
        "key_stored": true,
    }))
}

pub fn call(config: &Config, op: &str, params: Value) -> Result<Value> {
    let context = write_context(config)?;
    let result = match op {
        "apply_tags" => apply_tags(config, &context, &params),
        "move_to_collection" => move_to_collection(config, &context, &params),
        "create_note" => create_note(config, &context, &params),
        _ => Err(anyhow!("unsupported Zotero Local API operation: {op}")),
    };
    // Zotero consumes one-time keys as soon as a write authenticates, including
    // requests that subsequently fail validation in the endpoint handler.
    if !context.record.remember {
        let _ = fs::remove_file(&context.key_path);
    }
    result
}

fn apply_tags(config: &Config, context: &WriteContext, params: &Value) -> Result<Value> {
    let keys = string_array(params, "itemKeys")?;
    let add = string_array_or_empty(params, "addTags");
    let remove = string_array_or_empty(params, "removeTags");
    let mut results = Vec::new();
    for key in keys {
        let envelope = get_item(config, &key)?;
        let existing = envelope
            .get("data")
            .and_then(|data| data.get("tags"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let (tags, added, removed) = merge_tags(&existing, &add, &remove);
        if !added.is_empty() || !removed.is_empty() {
            patch_item(
                config,
                context,
                &key,
                json!({
                    "version": object_version(&envelope)?,
                    "tags": tags,
                }),
            )?;
        }
        let current = get_item(config, &key)?;
        let current_tags = current
            .pointer("/data/tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| tag.get("tag").and_then(Value::as_str))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let verified = add.iter().all(|tag| current_tags.contains(&tag.as_str()))
            && remove
                .iter()
                .all(|tag| !current_tags.contains(&tag.as_str()));
        if !verified {
            return Err(anyhow!(
                "Zotero Local API tag write could not be verified for {key}"
            ));
        }
        results.push(json!({"key": key, "added": added, "removed": removed, "verified": true}));
    }
    Ok(json!({
        "ok": true,
        "op": "apply_tags",
        "transport": "local_api",
        "count": results.len(),
        "items": results,
    }))
}

fn move_to_collection(config: &Config, context: &WriteContext, params: &Value) -> Result<Value> {
    let keys = string_array(params, "itemKeys")?;
    let collection_key = required_string(params, "collectionKey")?;
    validate_object_key(&collection_key)?;
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("add");
    let mut results = Vec::new();
    for key in keys {
        let envelope = get_item(config, &key)?;
        let mut collections = envelope
            .get("data")
            .and_then(|data| data.get("collections"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        let changed = if action == "remove" {
            let before = collections.len();
            collections.retain(|key| key != &collection_key);
            before != collections.len()
        } else if collections.iter().any(|key| key == &collection_key) {
            false
        } else {
            collections.push(collection_key.clone());
            true
        };
        if changed {
            patch_item(
                config,
                context,
                &key,
                json!({
                    "version": object_version(&envelope)?,
                    "collections": collections,
                }),
            )?;
        }
        let current = get_item(config, &key)?;
        let current_collections = current
            .pointer("/data/collections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let is_present = current_collections.contains(&collection_key.as_str());
        let verified = if action == "remove" {
            !is_present
        } else {
            is_present
        };
        if !verified {
            return Err(anyhow!(
                "Zotero Local API collection write could not be verified for {key}"
            ));
        }
        results.push(json!({
            "key": key,
            "collectionKey": collection_key,
            "action": action,
            "changed": changed,
            "verified": true,
        }));
    }
    Ok(json!({
        "ok": true,
        "op": "move_to_collection",
        "transport": "local_api",
        "count": results.len(),
        "items": results,
    }))
}

fn create_note(config: &Config, context: &WriteContext, params: &Value) -> Result<Value> {
    let parent_key = required_string(params, "itemKey")?;
    validate_object_key(&parent_key)?;
    let content = required_string(params, "content")?;
    let title = params.get("title").and_then(Value::as_str);
    let note = note_html(title, &content);
    let response = send_json(
        config,
        "POST",
        "users/0/items",
        &json!([{
            "itemType": "note",
            "parentItem": parent_key,
            "note": note,
        }]),
        Some(&context.record.server_id),
        Some(&context.record.key),
        true,
        Duration::from_secs(5),
    )?;
    ensure_success(&response, "create Zotero note")?;
    let value: Value = serde_json::from_str(&response.body)
        .context("Zotero Local API note response was invalid JSON")?;
    if let Some(failed) = value.get("failed").and_then(Value::as_object) {
        if !failed.is_empty() {
            return Err(anyhow!("Zotero Local API note creation failed: {failed:?}"));
        }
    }
    let key = value
        .pointer("/success/0")
        .and_then(Value::as_str)
        .or_else(|| value.pointer("/successful/0/key").and_then(Value::as_str));
    Ok(json!({
        "ok": true,
        "op": "create_note",
        "transport": "local_api",
        "count": 1,
        "item_key": key,
        "parent_key": parent_key,
    }))
}

fn patch_item(config: &Config, context: &WriteContext, key: &str, body: Value) -> Result<()> {
    validate_object_key(key)?;
    let response = send_json(
        config,
        "PATCH",
        &format!("users/0/items/{key}"),
        &body,
        Some(&context.record.server_id),
        Some(&context.record.key),
        true,
        Duration::from_secs(5),
    )?;
    ensure_success(&response, "update Zotero item")
}

fn get_item(config: &Config, key: &str) -> Result<Value> {
    validate_object_key(key)?;
    let response = get(
        config,
        &format!("users/0/items/{key}"),
        Duration::from_secs(3),
    )?;
    ensure_success(&response, "read Zotero item")?;
    serde_json::from_str(&response.body).context("Zotero Local API item response was invalid JSON")
}

fn write_context(config: &Config) -> Result<WriteContext> {
    if !config.local_api.enabled {
        return Err(anyhow!("Zotero Local API is disabled in zcli config"));
    }
    let key_path =
        key_path(config).ok_or_else(|| anyhow!("local API key path is not configured"))?;
    let record = load_key_record(Some(&key_path))?
        .ok_or_else(|| anyhow!("Zotero Local API is not authorized; run `zcli local-api authorize --dry-run`, then `--execute`"))?;
    let current = probe(config)?;
    let current_server_id = current
        .server_id
        .ok_or_else(|| anyhow!("Zotero Local API did not return Zotero-Server-ID"))?;
    if current_server_id != record.server_id {
        return Err(anyhow!(
            "Zotero Local API server changed; authorize this Zotero profile again"
        ));
    }
    Ok(WriteContext { record, key_path })
}

fn probe(config: &Config) -> Result<ApiResponse> {
    let response = get(config, "", Duration::from_secs(2))?;
    ensure_success(&response, "connect to Zotero Local API")?;
    Ok(response)
}

fn get(config: &Config, resource: &str, timeout: Duration) -> Result<ApiResponse> {
    let url = resource_url(config, resource);
    let mut response = client(timeout)
        .get(&url)
        .header("User-Agent", APP_NAME)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .header("Zotero-Allowed-Request", "1")
        .call()
        .with_context(|| format!("could not connect to Zotero Local API at {url}"))?;
    read_response(&mut response)
}

#[allow(clippy::too_many_arguments)]
fn send_json(
    config: &Config,
    method: &str,
    resource: &str,
    body: &Value,
    server_id: Option<&str>,
    api_key: Option<&str>,
    include_write_token: bool,
    timeout: Duration,
) -> Result<ApiResponse> {
    let url = resource_url(config, resource);
    let agent = client(timeout);
    let mut request = match method {
        "POST" => agent.post(&url),
        "PATCH" => agent.patch(&url),
        "PUT" => agent.put(&url),
        _ => return Err(anyhow!("unsupported Local API method: {method}")),
    }
    .header("User-Agent", APP_NAME)
    .header("Accept", "application/json")
    .header("Accept-Encoding", "identity")
    .header("Content-Type", "application/json")
    .header("Zotero-Allowed-Request", "1");
    if let Some(server_id) = server_id {
        request = request.header("Zotero-Server-ID", server_id);
    }
    if let Some(api_key) = api_key {
        request = request.header("Zotero-API-Key", api_key);
    }
    if include_write_token {
        request = request.header("Zotero-Write-Token", write_token());
    }
    let mut response = request
        .send(body.to_string())
        .with_context(|| format!("Zotero Local API request failed at {url}"))?;
    read_response(&mut response)
}

fn client(timeout: Duration) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

fn read_response(response: &mut Response<Body>) -> Result<ApiResponse> {
    let status = response.status().as_u16();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let server_id = header("Zotero-Server-ID");
    let zotero_version = header("X-Zotero-Version");
    let api_version = header("Zotero-API-Version");
    let schema_version = header("Zotero-Schema-Version");
    let body = response
        .body_mut()
        .read_to_string()
        .context("failed to read Zotero Local API response")?;
    Ok(ApiResponse {
        status,
        body,
        server_id,
        zotero_version,
        api_version,
        schema_version,
    })
}

fn ensure_success(response: &ApiResponse, action: &str) -> Result<()> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    let body = response.body.trim();
    Err(anyhow!(
        "failed to {action}: Zotero Local API HTTP {}{}",
        response.status,
        if body.is_empty() {
            String::new()
        } else {
            format!(": {body}")
        }
    ))
}

fn resource_url(config: &Config, resource: &str) -> String {
    let base = config.local_api.endpoint.trim_end_matches('/');
    let resource = resource.trim_start_matches('/');
    if resource.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{resource}")
    }
}

fn key_path(config: &Config) -> Option<PathBuf> {
    config.local_api.key_path.clone().or_else(|| {
        config
            .state_dir
            .as_ref()
            .map(|dir| dir.join("local-api-key.json"))
    })
}

fn load_key_record(path: Option<&Path>) -> Result<Option<LocalApiKeyRecord>> {
    let Some(path) = path else { return Ok(None) };
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read local API key record {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse local API key record {}", path.display()))?;
    Ok(Some(record))
}

fn save_key_record(path: &Path, record: &LocalApiKeyRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(record)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn object_version(value: &Value) -> Result<u64> {
    value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("Zotero Local API item response did not include version"))
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{key} is required"))
}

fn string_array(value: &Value, key: &str) -> Result<Vec<String>> {
    let values = string_array_or_empty(value, key);
    if values.is_empty() {
        return Err(anyhow!("{key} must contain at least one value"));
    }
    Ok(values)
}

fn string_array_or_empty(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn validate_object_key(key: &str) -> Result<()> {
    if key.len() != 8
        || !key
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
    {
        return Err(anyhow!("invalid Zotero object key: {key}"));
    }
    Ok(())
}

fn merge_tags(
    existing: &[Value],
    add: &[String],
    remove: &[String],
) -> (Vec<Value>, Vec<String>, Vec<String>) {
    let remove_set = remove.iter().collect::<std::collections::HashSet<_>>();
    let mut tags = Vec::new();
    let mut removed = Vec::new();
    for tag in existing {
        let name = tag.get("tag").and_then(Value::as_str).unwrap_or("");
        if remove_set
            .iter()
            .any(|candidate| candidate.as_str() == name)
        {
            if !removed.iter().any(|candidate| candidate == name) {
                removed.push(name.to_string());
            }
        } else {
            tags.push(tag.clone());
        }
    }
    let mut added = Vec::new();
    for name in add {
        if !tags
            .iter()
            .any(|tag| tag.get("tag").and_then(Value::as_str) == Some(name.as_str()))
        {
            tags.push(json!({"tag": name, "type": 0}));
            added.push(name.clone());
        }
    }
    (tags, added, removed)
}

fn note_html(title: Option<&str>, content: &str) -> String {
    let body = if content.contains("<p") || content.contains("<div") || content.contains("<h") {
        content.to_string()
    } else {
        format!("<p>{}</p>", escape_html(content).replace('\n', "<br/>"))
    };
    match title.map(str::trim).filter(|title| !title.is_empty()) {
        Some(title) => format!("<h1>{}</h1>{body}", escape_html(title)),
        None => body,
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn write_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("zcli{}{}", std::process::id(), now)
        .chars()
        .take(32)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        os::unix::fs::PermissionsExt,
        thread,
    };

    use super::*;

    fn spawn_server(
        responses: Vec<(u16, Vec<(&'static str, &'static str)>, String)>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, headers, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                let header_end = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0, "client closed before sending HTTP headers");
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let header_text = String::from_utf8_lossy(&request[..header_end]);
                let content_length = header_text
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0, "client closed before sending HTTP body");
                    request.extend_from_slice(&buffer[..count]);
                }
                requests.push(String::from_utf8(request).unwrap());

                let reason = if status == 204 { "No Content" } else { "OK" };
                let mut response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&body);
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}/api"), handle)
    }

    fn test_config(endpoint: String, key_path: PathBuf) -> Config {
        let mut config = Config::default();
        config.local_api.endpoint = endpoint;
        config.local_api.key_path = Some(key_path);
        config
    }

    #[test]
    fn merges_tags_without_losing_existing_tag_types() {
        let existing = vec![
            json!({"tag": "keep", "type": 1}),
            json!({"tag": "remove", "type": 0}),
        ];
        let (tags, added, removed) = merge_tags(
            &existing,
            &["new".to_string(), "keep".to_string()],
            &["remove".to_string()],
        );
        assert_eq!(added, vec!["new"]);
        assert_eq!(removed, vec!["remove"]);
        assert_eq!(
            tags,
            vec![
                json!({"tag": "keep", "type": 1}),
                json!({"tag": "new", "type": 0})
            ]
        );
    }

    #[test]
    fn formats_plain_text_notes_like_the_helper() {
        assert_eq!(
            note_html(Some("Title & More"), "one < two\nnext"),
            "<h1>Title &amp; More</h1><p>one &lt; two<br/>next</p>"
        );
    }

    #[test]
    fn tag_write_uses_probe_version_and_authenticated_patch() {
        let responses = vec![
            (
                200,
                vec![
                    ("Zotero-Server-ID", "server-1"),
                    ("X-Zotero-Version", "10.0"),
                    ("Zotero-API-Version", "3"),
                ],
                "{}".to_string(),
            ),
            (
                200,
                vec![],
                json!({
                    "key": "ITEM0001",
                    "version": 17,
                    "data": {"tags": [{"tag": "keep", "type": 1}]}
                })
                .to_string(),
            ),
            (204, vec![], String::new()),
            (
                200,
                vec![],
                json!({
                    "key": "ITEM0001",
                    "version": 18,
                    "data": {"tags": [
                        {"tag": "keep", "type": 1},
                        {"tag": "new", "type": 0}
                    ]}
                })
                .to_string(),
            ),
        ];
        let (endpoint, server) = spawn_server(responses);
        let temp = tempfile::tempdir().unwrap();
        let key_path = temp.path().join("local-api-key.json");
        save_key_record(
            &key_path,
            &LocalApiKeyRecord {
                key: "test-key".to_string(),
                server_id: "server-1".to_string(),
                remember: true,
            },
        )
        .unwrap();
        let config = test_config(endpoint, key_path);

        let result = call(
            &config,
            "apply_tags",
            json!({"itemKeys": ["ITEM0001"], "addTags": ["new"], "removeTags": []}),
        )
        .unwrap();
        assert_eq!(result["items"][0]["added"], json!(["new"]));
        assert_eq!(result["items"][0]["verified"], true);

        let requests = server.join().unwrap();
        let probe = requests[0].to_ascii_lowercase();
        assert!(probe.starts_with("get /api/ http/1.1"));
        assert!(probe.contains("zotero-allowed-request: 1"));
        let patch = requests[2].to_ascii_lowercase();
        assert!(patch.starts_with("patch /api/users/0/items/item0001 http/1.1"));
        assert!(patch.contains("zotero-server-id: server-1"));
        assert!(patch.contains("zotero-api-key: test-key"));
        assert!(patch.contains("zotero-write-token:"));
        let body: Value =
            serde_json::from_str(requests[2].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["version"], 17);
        assert_eq!(
            body["tags"],
            json!([{"tag": "keep", "type": 1}, {"tag": "new", "type": 0}])
        );
    }

    #[test]
    fn authorization_redacts_key_and_stores_it_with_owner_only_permissions() {
        let responses = vec![
            (
                200,
                vec![("Zotero-Server-ID", "server-2")],
                "{}".to_string(),
            ),
            (
                200,
                vec![],
                json!({"key": "private-local-api-key", "remember": true}).to_string(),
            ),
        ];
        let (endpoint, server) = spawn_server(responses);
        let temp = tempfile::tempdir().unwrap();
        let key_path = temp.path().join("local-api-key.json");
        let config = test_config(endpoint, key_path.clone());

        let result = authorize(&config, false, true).unwrap();
        let rendered = serde_json::to_string(&result).unwrap();
        assert!(!rendered.contains("private-local-api-key"));
        assert_eq!(result["key_stored"], true);
        assert_eq!(
            fs::metadata(&key_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let requests = server.join().unwrap();
        let authorize_request = requests[1].to_ascii_lowercase();
        assert!(authorize_request.starts_with("post /api/local/authorize http/1.1"));
        assert!(authorize_request.contains("zotero-server-id: server-2"));
        assert!(!authorize_request.contains("zotero-api-key:"));
    }

    #[test]
    fn one_time_key_is_removed_even_when_operation_fails() {
        let responses = vec![(
            200,
            vec![("Zotero-Server-ID", "server-3")],
            "{}".to_string(),
        )];
        let (endpoint, server) = spawn_server(responses);
        let temp = tempfile::tempdir().unwrap();
        let key_path = temp.path().join("local-api-key.json");
        save_key_record(
            &key_path,
            &LocalApiKeyRecord {
                key: "one-time-key".to_string(),
                server_id: "server-3".to_string(),
                remember: false,
            },
        )
        .unwrap();
        let config = test_config(endpoint, key_path.clone());

        assert!(call(&config, "unsupported", json!({})).is_err());
        assert!(!key_path.exists());
        server.join().unwrap();
    }
}
