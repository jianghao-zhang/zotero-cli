use std::{env, time::Duration};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use ureq::{http::Response, Agent, Body};

use crate::config::{Config, WebApiConfig};

const APP_NAME: &str = "zotero-cli";

#[derive(Debug)]
struct ApiResponse {
    status: u16,
    body: String,
    total_results: Option<String>,
    last_modified_version: Option<String>,
}

pub fn doctor(config: &Config) -> Result<Value> {
    let web = &config.web_api;
    let mut base = json!({
        "ok": true,
        "enabled": web.enabled,
        "base_url": web.base_url,
        "library_type": web.library_type,
        "library_id": web.library_id,
        "api_key_env": web.api_key_env,
        "network_used": false,
        "writes_executed": false,
    });
    if !web.enabled {
        base["status"] = json!("disabled");
        return Ok(base);
    }

    let Some(prefix) = library_prefix(web) else {
        base["status"] = json!("configuration_incomplete");
        base["error"] = json!("library_type must be user or group and library_id must be set");
        return Ok(base);
    };
    let Some(api_key) = api_key(web) else {
        base["status"] = json!("api_key_missing");
        base["error"] = json!(
            "configure api_key_env or store a key with `zcli config web-api --api-key-stdin`"
        );
        return Ok(base);
    };

    base["network_used"] = json!(true);
    base["library_endpoint"] = json!(format!("{}/{prefix}", web.base_url.trim_end_matches('/')));

    let key_response = match get(web, "keys/current", &api_key) {
        Ok(response) => response,
        Err(error) => {
            base["status"] = json!("unavailable");
            base["error"] = json!(error.to_string());
            return Ok(base);
        }
    };
    if key_response.status != 200 {
        base["status"] = json!(status_label(key_response.status));
        base["http_status"] = json!(key_response.status);
        return Ok(base);
    }
    let key_info: Value = serde_json::from_str(&key_response.body)
        .context("Zotero Web API key metadata was invalid JSON")?;
    base["key"] = sanitized_key_info(&key_info, web);

    let library_response = match get(
        web,
        &format!("{prefix}/items?limit=1&format=versions"),
        &api_key,
    ) {
        Ok(response) => response,
        Err(error) => {
            base["status"] = json!("unavailable");
            base["error"] = json!(error.to_string());
            return Ok(base);
        }
    };
    base["http_status"] = json!(library_response.status);
    base["total_results"] = library_response
        .total_results
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Value::from)
        .unwrap_or(Value::Null);
    base["last_modified_version"] = library_response
        .last_modified_version
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Value::from)
        .unwrap_or(Value::Null);
    base["status"] = json!(if library_response.status == 200 {
        "available_authorized"
    } else {
        status_label(library_response.status)
    });
    Ok(base)
}

fn library_prefix(config: &WebApiConfig) -> Option<String> {
    let id = config.library_id.as_deref()?.trim();
    if id.is_empty() || !id.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    match config.library_type.trim() {
        "user" => Some(format!("users/{id}")),
        "group" => Some(format!("groups/{id}")),
        _ => None,
    }
}

fn api_key(config: &WebApiConfig) -> Option<String> {
    config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .or_else(|| {
            config
                .api_key_env
                .as_deref()
                .and_then(|name| env::var(name).ok())
                .map(|key| key.trim().to_string())
                .filter(|key| !key.is_empty())
        })
}

fn get(config: &WebApiConfig, resource: &str, api_key: &str) -> Result<ApiResponse> {
    let url = format!(
        "{}/{}",
        config.base_url.trim_end_matches('/'),
        resource.trim_start_matches('/')
    );
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("User-Agent", APP_NAME)
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .header("Zotero-API-Version", "3")
        .header("Zotero-API-Key", api_key)
        .call()
        .with_context(|| format!("could not connect to Zotero Web API at {url}"))?;
    read_response(&mut response)
}

fn read_response(response: &mut Response<Body>) -> Result<ApiResponse> {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let total_results = header("Total-Results");
    let last_modified_version = header("Last-Modified-Version");
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .context("failed to read Zotero Web API response")?;
    Ok(ApiResponse {
        status,
        body,
        total_results,
        last_modified_version,
    })
}

fn status_label(status: u16) -> &'static str {
    match status {
        401 => "unauthorized",
        403 => "forbidden",
        404 => "library_not_found",
        429 => "rate_limited",
        _ => "http_error",
    }
}

fn sanitized_key_info(value: &Value, config: &WebApiConfig) -> Value {
    let access = value.get("access").cloned().unwrap_or(Value::Null);
    let configured_access = match config.library_type.as_str() {
        "user" => access.get("user").cloned().unwrap_or(Value::Null),
        "group" => config
            .library_id
            .as_deref()
            .and_then(|id| {
                access
                    .pointer(&format!("/groups/{id}"))
                    .or_else(|| access.pointer("/groups/all"))
            })
            .cloned()
            .unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let identity_matches_library = if config.library_type == "user" {
        let configured_id = config.library_id.as_deref();
        let key_id = value.get("userID").and_then(|id| {
            id.as_u64()
                .map(|id| id.to_string())
                .or_else(|| id.as_str().map(str::to_string))
        });
        configured_id
            .zip(key_id.as_deref())
            .map(|(left, right)| left == right)
    } else {
        None
    };
    json!({
        "identity_matches_library": identity_matches_library,
        "configured_library_access": configured_access,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    fn spawn_server() -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            let responses = [
                json!({
                    "userID": 123,
                    "username": "researcher",
                    "access": {"user": {"library": true, "files": true, "write": true}}
                })
                .to_string(),
                "{}".to_string(),
            ];
            for (index, body) in responses.into_iter().enumerate() {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(request).unwrap());
                let extra = if index == 1 {
                    "Total-Results: 42\r\nLast-Modified-Version: 17\r\n"
                } else {
                    ""
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn doctor_validates_key_and_library_without_writing() {
        let (base_url, server) = spawn_server();
        let mut config = Config::default();
        config.web_api.enabled = true;
        config.web_api.base_url = base_url;
        config.web_api.library_type = "user".to_string();
        config.web_api.library_id = Some("123".to_string());
        config.web_api.api_key = Some("secret".to_string());

        let value = doctor(&config).unwrap();
        assert_eq!(value["status"], "available_authorized");
        assert_eq!(value["total_results"], 42);
        assert_eq!(value["last_modified_version"], 17);
        assert_eq!(value["writes_executed"], false);
        assert_eq!(value["key"]["configured_library_access"]["write"], true);
        assert_eq!(value["key"]["identity_matches_library"], true);

        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /keys/current HTTP/1.1"));
        assert!(requests[0]
            .to_ascii_lowercase()
            .contains("zotero-api-key: secret"));
        assert!(requests[1].starts_with("GET /users/123/items?limit=1&format=versions HTTP/1.1"));
    }

    #[test]
    fn doctor_reports_incomplete_config_without_network() {
        let mut config = Config::default();
        config.web_api.enabled = true;
        assert_eq!(
            doctor(&config).unwrap()["status"],
            "configuration_incomplete"
        );
    }
}
