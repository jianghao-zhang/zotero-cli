use std::{fs, path::PathBuf, process::Command, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, Utc};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Value};
use ureq::Agent;

use crate::import_plan;

const API_BASE: &str = "https://api.alphaxiv.org";
const WEB_BASE: &str = "https://www.alphaxiv.org";
const PDF_BASE: &str = "https://fetcher.alphaxiv.org/v2/pdf";
const CLERK_BASE: &str = "https://clerk.alphaxiv.org";
const CLERK_API_VERSION: &str = "2025-04-10";
const CLERK_JS_VERSION: &str = "5.125.10";

#[derive(Debug, Subcommand)]
pub enum AlphaXivCommands {
    AuthStatus(AuthStatusArgs),
    AuthRefresh(AuthRefreshArgs),
    Feed(FeedArgs),
    Search(SearchArgs),
    Discover(DiscoverArgs),
    Brief(BriefArgs),
    Paper(PaperArgs),
    Overview(OverviewArgs),
    Markdown(MarkdownArgs),
    Pdf(PdfArgs),
    ZoteroPlan(ZoteroPlanArgs),
}

#[derive(Debug, Args)]
pub struct AuthStatusArgs {
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct AuthRefreshArgs {
    #[arg(long, default_value_t = 5)]
    pub limit: usize,
    #[arg(long, default_value = "7 Days")]
    pub interval: String,
    #[arg(long)]
    pub no_open: bool,
    #[arg(long)]
    pub no_validate_feed: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FeedSort {
    Hot,
    Comments,
    Views,
    Likes,
    Github,
    Twitter,
    Recommended,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DateField {
    FirstSeen,
    Published,
    Updated,
    Any,
}

impl DateField {
    fn as_str(self) -> &'static str {
        match self {
            Self::FirstSeen => "first_seen",
            Self::Published => "published",
            Self::Updated => "updated",
            Self::Any => "any",
        }
    }
}

impl FeedSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Hot => "Hot",
            Self::Comments => "Comments",
            Self::Views => "Views",
            Self::Likes => "Likes",
            Self::Github => "GitHub",
            Self::Twitter => "Twitter",
            Self::Recommended => "Recommended",
        }
    }
}

#[derive(Debug, Args)]
pub struct FeedArgs {
    #[arg(long, value_enum, default_value = "hot")]
    pub sort: FeedSort,
    #[arg(long, default_value = "All time")]
    pub interval: String,
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    #[arg(long, default_value_t = 100)]
    pub page_size: usize,
    #[arg(long = "topic")]
    pub topics: Vec<String>,
    #[arg(long)]
    pub min_likes: Option<i64>,
    #[arg(long)]
    pub min_github_stars: Option<i64>,
    #[arg(long)]
    pub min_visits: Option<i64>,
    #[arg(long)]
    pub rank_metrics: bool,
    #[arg(long)]
    pub with_zotero_plan: bool,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub days: Option<i64>,
    #[arg(long, value_enum, default_value = "first-seen")]
    pub date_field: DateField,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    #[arg(long)]
    pub fast: bool,
    #[arg(long = "topic")]
    pub topics: Vec<String>,
    #[arg(long)]
    pub min_likes: Option<i64>,
    #[arg(long)]
    pub min_github_stars: Option<i64>,
    #[arg(long)]
    pub min_visits: Option<i64>,
    #[arg(long)]
    pub rank_metrics: bool,
    #[arg(long)]
    pub with_zotero_plan: bool,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub days: Option<i64>,
    #[arg(long, value_enum, default_value = "first-seen")]
    pub date_field: DateField,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct DiscoverArgs {
    pub query: String,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, value_enum, default_value = "hot")]
    pub fallback_sort: FeedSort,
    #[arg(long, default_value = "30 Days")]
    pub fallback_interval: String,
    #[arg(long = "topic")]
    pub topics: Vec<String>,
    #[arg(long)]
    pub min_likes: Option<i64>,
    #[arg(long)]
    pub min_github_stars: Option<i64>,
    #[arg(long)]
    pub min_visits: Option<i64>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub days: Option<i64>,
    #[arg(long, value_enum, default_value = "first-seen")]
    pub date_field: DateField,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct BriefArgs {
    pub query: String,
    #[arg(long, default_value_t = 8)]
    pub limit: usize,
    #[arg(long, value_enum, default_value = "hot")]
    pub fallback_sort: FeedSort,
    #[arg(long, default_value = "30 Days")]
    pub fallback_interval: String,
    #[arg(long = "topic")]
    pub topics: Vec<String>,
    #[arg(long)]
    pub min_likes: Option<i64>,
    #[arg(long)]
    pub min_github_stars: Option<i64>,
    #[arg(long)]
    pub min_visits: Option<i64>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub days: Option<i64>,
    #[arg(long, value_enum, default_value = "any")]
    pub date_field: DateField,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

struct DiscoveryPipeline<'a> {
    query: &'a str,
    limit: usize,
    fallback_sort: FeedSort,
    fallback_interval: &'a str,
    topics: &'a [String],
    min_likes: Option<i64>,
    min_github_stars: Option<i64>,
    min_visits: Option<i64>,
    cutoff: Option<DateTime<Utc>>,
    date_field: DateField,
    triage: bool,
}

#[derive(Debug, Args)]
pub struct PaperArgs {
    pub paper_id: String,
    #[arg(long)]
    pub raw: bool,
    #[arg(long)]
    pub no_preview: bool,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct OverviewArgs {
    pub paper_id: String,
    #[arg(long, default_value = "en")]
    pub lang: String,
    #[arg(long)]
    pub status: bool,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MarkdownKind {
    Abs,
    Overview,
}

#[derive(Debug, Args)]
pub struct MarkdownArgs {
    pub paper_id: String,
    #[arg(long, value_enum, default_value = "abs")]
    pub kind: MarkdownKind,
    #[arg(long, default_value = "json")]
    pub format: String,
    #[arg(long, default_value_t = 0)]
    pub max_chars: usize,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct PdfArgs {
    pub paper_id: String,
    #[arg(long)]
    pub download: Option<PathBuf>,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

#[derive(Debug, Args)]
pub struct ZoteroPlanArgs {
    pub paper_id: String,
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
}

pub fn dispatch(command: &AlphaXivCommands) -> Result<Value> {
    match command {
        AlphaXivCommands::AuthStatus(args) => auth_status(args),
        AlphaXivCommands::AuthRefresh(args) => auth_refresh(args),
        AlphaXivCommands::Feed(args) => feed(args),
        AlphaXivCommands::Search(args) => search(args),
        AlphaXivCommands::Discover(args) => discover(args),
        AlphaXivCommands::Brief(args) => brief(args),
        AlphaXivCommands::Paper(args) => paper(args),
        AlphaXivCommands::Overview(args) => overview(args),
        AlphaXivCommands::Markdown(args) => markdown(args),
        AlphaXivCommands::Pdf(args) => pdf(args),
        AlphaXivCommands::ZoteroPlan(args) => zotero_plan(args),
    }
}

fn client(timeout: u64) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("alphaxiv")
}

fn default_cookie_file() -> PathBuf {
    config_dir().join("clerk-cookie.txt")
}

fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

fn read_first_data_line(path: &PathBuf) -> Result<Option<String>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("PASTE_"))
        .map(ToOwned::to_owned))
}

fn clerk_cookie_header() -> Result<Option<String>> {
    if let Ok(value) = std::env::var("ALPHAXIV_CLERK_COOKIE") {
        if !value.trim().is_empty() {
            return Ok(Some(value.trim().to_string()));
        }
    }
    let path = std::env::var_os("ALPHAXIV_CLERK_COOKIE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(default_cookie_file);
    if !path.exists() {
        return Ok(None);
    }
    let cookie = read_first_data_line(&path)?;
    Ok(cookie.map(|line| {
        line.strip_prefix("Cookie:")
            .or_else(|| line.strip_prefix("cookie:"))
            .unwrap_or(&line)
            .trim()
            .to_string()
    }))
}

fn request_json(
    client: &Agent,
    path_or_url: &str,
    params: &[(&str, String)],
    auth: bool,
) -> Result<Value> {
    let url = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        path_or_url.to_string()
    } else {
        format!("{API_BASE}{path_or_url}")
    };
    let mut request = client
        .get(url)
        .query_pairs(params.iter().map(|(key, value)| (*key, value.as_str())))
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "application/json,*/*")
        .header("Accept-Encoding", "identity")
        .header("Origin", WEB_BASE)
        .header("Referer", format!("{WEB_BASE}/"));
    if auth {
        if let Some(token) = authorization_header(client)? {
            request = request.header("Authorization", token);
        }
    }
    let mut response = request.call().context("alphaXiv request failed")?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .context("failed to read alphaXiv response")?;
    if !status.is_success() {
        bail!("alphaXiv HTTP {}: {}", status.as_u16(), text.trim());
    }
    serde_json::from_str(&text).context("alphaXiv returned non-JSON response")
}

fn request_text(client: &Agent, url: &str) -> Result<String> {
    let mut response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "text/markdown,*/*")
        .header("Accept-Encoding", "identity")
        .header("Origin", WEB_BASE)
        .header("Referer", format!("{WEB_BASE}/"))
        .call()
        .context("alphaXiv text request failed")?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .context("failed to read alphaXiv text response")?;
    if !status.is_success() {
        bail!("alphaXiv HTTP {}: {}", status.as_u16(), text.trim());
    }
    Ok(text)
}

fn request_bytes(client: &Agent, url: &str) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "application/pdf,*/*")
        .header("Accept-Encoding", "identity")
        .header("Origin", WEB_BASE)
        .header("Referer", format!("{WEB_BASE}/"))
        .call()
        .context("alphaXiv PDF request failed")?;
    let status = response.status();
    let bytes = response
        .body_mut()
        .read_to_vec()
        .context("failed to read alphaXiv PDF response")?;
    if !status.is_success() {
        bail!(
            "alphaXiv HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&bytes)
        );
    }
    Ok(bytes.to_vec())
}

fn clerk_client(client: &Agent) -> Result<Value> {
    let cookie = clerk_cookie_header()?.ok_or_else(|| {
        anyhow!(
            "No Clerk session cookie configured. Export it to ~/.config/alphaxiv/clerk-cookie.txt."
        )
    })?;
    let mut response = client
        .get(format!("{CLERK_BASE}/v1/client"))
        .query_pairs([
            ("__clerk_api_version", CLERK_API_VERSION),
            ("_clerk_js_version", CLERK_JS_VERSION),
        ])
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "*/*")
        .header("Accept-Encoding", "identity")
        .header("Origin", WEB_BASE)
        .header("Referer", format!("{WEB_BASE}/"))
        .header("Cookie", cookie)
        .call()
        .context("Clerk request failed")?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .context("failed to read Clerk response")?;
    if !status.is_success() {
        bail!("Clerk HTTP {}: {}", status.as_u16(), text.trim());
    }
    serde_json::from_str(&text).context("Clerk returned non-JSON response")
}

fn active_clerk_session(data: &Value) -> Value {
    data.pointer("/response/sessions")
        .and_then(Value::as_array)
        .and_then(|sessions| {
            sessions
                .iter()
                .find(|s| s.get("status").and_then(Value::as_str) == Some("active"))
                .or_else(|| sessions.first())
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn authorization_header(client: &Agent) -> Result<Option<String>> {
    if let Ok(value) =
        std::env::var("ALPHAXIV_AUTHORIZATION").or_else(|_| std::env::var("ALPHAXIV_BEARER_TOKEN"))
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(Some(
                if trimmed.to_ascii_lowercase().starts_with("bearer ") {
                    trimmed.to_string()
                } else {
                    format!("Bearer {trimmed}")
                },
            ));
        }
    }
    let session = active_clerk_session(&clerk_client(client)?);
    let jwt = session
        .pointer("/last_active_token/jwt")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    Ok(jwt.map(|token| format!("Bearer {token}")))
}

fn jwt_payload(jwt: &str) -> Value {
    let token = jwt.strip_prefix("Bearer ").unwrap_or(jwt);
    let Some(payload) = token.split('.').nth(1) else {
        return Value::Null;
    };
    URL_SAFE_NO_PAD
        .decode(payload.as_bytes())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}

fn iso_from_ms(value: Option<i64>) -> Value {
    value
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|dt| json!(dt.to_rfc3339()))
        .unwrap_or(Value::Null)
}

fn iso_from_s(value: Option<i64>) -> Value {
    value
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
        .map(|dt| json!(dt.to_rfc3339()))
        .unwrap_or(Value::Null)
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    let without_timezone_name = value.split(" (").next().unwrap_or(value);
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            DateTime::parse_from_str(without_timezone_name, "%a %b %d %Y %H:%M:%S GMT%z")
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        })
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|dt| dt.and_utc())
        })
}

fn normalize_datetime(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_str)
        .and_then(parse_datetime)
        .map(|dt| json!(dt.to_rfc3339()))
        .unwrap_or(Value::Null)
}

fn since_cutoff(since: &Option<String>, days: Option<i64>) -> Result<Option<DateTime<Utc>>> {
    if let Some(raw) = since {
        return parse_datetime(raw)
            .map(Some)
            .ok_or_else(|| anyhow!("could not parse --since as YYYY-MM-DD or RFC3339: {raw}"));
    }
    Ok(days.map(|days| Utc::now() - ChronoDuration::days(days)))
}

fn first_present<'a>(values: &[Option<&'a Value>]) -> Option<&'a Value> {
    values.iter().flatten().copied().find(|value| match value {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    })
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(|v| match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn normalize_people(value: Option<&Value>) -> Value {
    let Some(array) = value.and_then(Value::as_array) else {
        return json!([]);
    };
    Value::Array(
        array
            .iter()
            .map(|person| {
                if let Some(name) = person.as_str() {
                    return json!(name);
                }
                if let Some(object) = person.as_object() {
                    if let Some(name) = first_present(&[
                        object.get("name"),
                        object.get("full_name"),
                        object.get("display_name"),
                    ])
                    .and_then(Value::as_str)
                    {
                        return json!(name);
                    }
                }
                person.clone()
            })
            .collect(),
    )
}

fn normalize_topics(value: Option<&Value>) -> Value {
    let Some(array) = value.and_then(Value::as_array) else {
        return json!([]);
    };
    Value::Array(
        array
            .iter()
            .map(|topic| {
                if let Some(name) = topic.as_str() {
                    return json!(name);
                }
                if let Some(object) = topic.as_object() {
                    if let Some(name) = first_present(&[
                        object.get("name"),
                        object.get("display_name"),
                        object.get("slug"),
                    ])
                    .and_then(Value::as_str)
                    {
                        return json!(name);
                    }
                }
                topic.clone()
            })
            .collect(),
    )
}

fn resource_github(resources: Option<&Value>) -> Value {
    match resources {
        Some(Value::Object(map)) => {
            let value = first_present(&[
                map.get("github"),
                map.get("GitHub"),
                map.get("github_url"),
                map.get("url"),
            ])
            .cloned()
            .unwrap_or(Value::Null);
            if let Some(url) = value.get("url") {
                url.clone()
            } else {
                value
            }
        }
        Some(Value::Array(array)) => array
            .iter()
            .find_map(|item| {
                if let Some(s) = item.as_str() {
                    return s.contains("github.com").then(|| json!(s));
                }
                let object = item.as_object()?;
                let label =
                    first_present(&[object.get("type"), object.get("label"), object.get("name")])
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                let url = first_present(&[object.get("url"), object.get("link")])?;
                label.contains("github").then(|| url.clone())
            })
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn metrics_from(paper: &Value) -> Value {
    let metrics = paper.get("metrics").unwrap_or(&Value::Null);
    let visits = metrics.get("visits_count").unwrap_or(&Value::Null);
    let resources = paper.get("resources").unwrap_or(&Value::Null);
    let github_resource_stars = resources
        .get("github")
        .and_then(|github| github.get("stars"))
        .cloned();
    json!({
        "public_total_votes": metrics.get("public_total_votes").cloned().unwrap_or(Value::Null),
        "total_votes": metrics.get("total_votes").cloned().unwrap_or(Value::Null),
        "x_likes": metrics.get("x_likes").cloned().unwrap_or(Value::Null),
        "github_stars": first_present(&[paper.get("github_stars"), metrics.get("github_stars"), github_resource_stars.as_ref()]).cloned().unwrap_or(Value::Null),
        "visits_all": visits.get("all").cloned().unwrap_or(Value::Null),
        "visits_7d": visits.get("last_7_days").cloned().unwrap_or(Value::Null),
    })
}

fn is_versioned_id(value: &str) -> bool {
    let Some((base, version)) = value.rsplit_once('v') else {
        return false;
    };
    !base.is_empty() && !version.is_empty() && version.chars().all(|c| c.is_ascii_digit())
}

fn is_arxiv_base_id(value: &str) -> bool {
    let Some((prefix, suffix)) = value.split_once('.') else {
        return false;
    };
    prefix.len() == 4
        && (suffix.len() == 4 || suffix.len() == 5)
        && prefix.chars().all(|c| c.is_ascii_digit())
        && suffix.chars().all(|c| c.is_ascii_digit())
}

fn arxiv_base_id(value: &str) -> Option<String> {
    let base = if is_versioned_id(value) {
        value
            .rsplit_once('v')
            .map(|(base, _)| base)
            .unwrap_or(value)
    } else {
        value
    };
    is_arxiv_base_id(base).then(|| base.to_string())
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|&idx| bytes[idx] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(idx, byte)| [8, 13, 18, 23].contains(&idx) || byte.is_ascii_hexdigit())
}

fn paper_id_from_input(input: &str) -> String {
    let trimmed = input.trim();
    if let Some((_, tail)) = trimmed.split_once("alphaxiv.org/abs/") {
        return tail
            .split(['?', '#', '/'])
            .next()
            .unwrap_or(tail)
            .trim_end_matches(".md")
            .to_string();
    }
    if let Some((_, tail)) = trimmed.split_once("alphaxiv.org/overview/") {
        return tail
            .split(['?', '#', '/'])
            .next()
            .unwrap_or(tail)
            .trim_end_matches(".md")
            .to_string();
    }
    if let Some((_, tail)) = trimmed.split_once("fetcher.alphaxiv.org/v2/pdf/") {
        return tail
            .split(['?', '#', '/'])
            .next()
            .unwrap_or(tail)
            .to_string();
    }
    if let Some((_, tail)) = trimmed.split_once("arxiv.org/abs/") {
        return tail
            .split(['?', '#', '/'])
            .next()
            .unwrap_or(tail)
            .to_string();
    }
    if let Some((_, tail)) = trimmed.split_once("arxiv.org/pdf/") {
        return tail
            .split(['?', '#', '/'])
            .next()
            .unwrap_or(tail)
            .trim_end_matches(".pdf")
            .to_string();
    }
    trimmed.to_string()
}

fn normalize_flat(paper: &Value) -> Value {
    let universal_id = string_value(first_present(&[
        paper.get("universal_paper_id"),
        paper.get("universalId"),
        paper.get("universal_id"),
        paper.get("paperId"),
        paper
            .get("id")
            .filter(|id| id.as_str().map(|s| !is_uuid(s)).unwrap_or(true)),
    ]));
    let version_label = string_value(first_present(&[
        paper.get("versionLabel"),
        paper.get("version_label"),
    ]));
    let canonical_id = string_value(first_present(&[
        paper.get("canonical_id"),
        paper.get("canonicalId"),
    ]))
    .or_else(|| {
        universal_id
            .as_ref()
            .zip(version_label.as_ref())
            .map(|(id, v)| format!("{id}{v}"))
    })
    .or_else(|| {
        universal_id
            .as_ref()
            .filter(|id| is_versioned_id(id))
            .cloned()
    });
    let alphaxiv_id = universal_id.as_ref().map(|id| {
        if is_versioned_id(id) {
            arxiv_base_id(id).unwrap_or_else(|| id.clone())
        } else {
            id.clone()
        }
    });
    let resources = paper.get("resources").unwrap_or(&Value::Null);
    let github = first_present(&[
        paper.get("github_url"),
        Some(&resource_github(Some(resources))),
    ])
    .cloned()
    .unwrap_or(Value::Null);
    json!({
        "source": "alphaxiv",
        "title": paper.get("title").cloned().unwrap_or(Value::Null),
        "alphaxiv_id": alphaxiv_id,
        "canonical_id": canonical_id,
        "version_id": first_present(&[paper.get("version_id"), paper.get("versionId")]).cloned().unwrap_or(Value::Null),
        "group_id": first_present(&[
            paper.get("paper_group_id"),
            paper.get("groupId"),
            paper.get("group_id"),
            paper
                .get("id")
                .filter(|id| id.as_str().map(is_uuid).unwrap_or(false)),
        ]).cloned().unwrap_or(Value::Null),
        "url": alphaxiv_id.as_ref().map(|id| format!("{WEB_BASE}/abs/{id}")),
        "overview_url": alphaxiv_id.as_ref().map(|id| format!("{WEB_BASE}/overview/{id}")),
        "pdf_url": canonical_id.as_ref().map(|id| format!("{PDF_BASE}/{id}")),
        "authors": normalize_people(paper.get("authors")),
        "topics": normalize_topics(paper.get("topics")),
        "summary": first_present(&[paper.get("summary"), paper.get("abstract"), paper.get("paper_summary"), paper.get("overview")]).cloned().unwrap_or(Value::Null),
        "first_seen_at": normalize_datetime(first_present(&[
            paper.get("first_publication_date"),
            paper.get("firstPublicationDate"),
            paper.get("first_seen_at"),
        ])),
        "published_at": normalize_datetime(first_present(&[
            paper.get("publication_date"),
            paper.get("publicationDate"),
            paper.get("published_at"),
            paper.get("publishedAt"),
        ])),
        "updated_at": normalize_datetime(first_present(&[
            paper.get("updated_at"),
            paper.get("updatedAt"),
        ])),
        "metrics": metrics_from(paper),
        "resources": if github.is_null() { json!({}) } else { json!({ "github": github }) },
    })
}

fn normalize_paper(data: &Value) -> Value {
    if let Some(root) = data.get("paper").or_else(|| data.get("paper_group")) {
        let version = root
            .get("paper_version")
            .or_else(|| root.get("paperVersion"))
            .unwrap_or(&Value::Null);
        let group = root
            .get("paper_group")
            .or_else(|| root.get("paperGroup"))
            .unwrap_or(root);
        let mut merged = group.clone();
        if let (Some(dst), Some(src)) = (merged.as_object_mut(), version.as_object()) {
            for (key, value) in src {
                dst.entry(key.clone()).or_insert_with(|| value.clone());
            }
            if let Some(id) = src.get("id") {
                dst.insert("version_id".to_string(), id.clone());
            }
        }
        if let (Some(dst), Some(src)) = (merged.as_object_mut(), group.as_object()) {
            if let Some(id) = src.get("id") {
                dst.insert("paper_group_id".to_string(), id.clone());
            }
            if !dst.contains_key("resources") {
                if let Some(resources) = src.get("resources") {
                    dst.insert("resources".to_string(), resources.clone());
                }
            }
        }
        return normalize_flat(&merged);
    }
    normalize_flat(data)
}

fn merge_normalized(base: Value, extra: Value) -> Value {
    let mut base = base;
    let (Some(base_map), Some(extra_map)) = (base.as_object_mut(), extra.as_object()) else {
        return base;
    };
    for (key, value) in extra_map {
        match key.as_str() {
            "metrics" => {
                let entry = base_map.entry(key.clone()).or_insert_with(|| json!({}));
                if let (Some(dst), Some(src)) = (entry.as_object_mut(), value.as_object()) {
                    for (metric_key, metric_value) in src {
                        if !metric_value.is_null() && dst.get(metric_key).is_none_or(Value::is_null)
                        {
                            dst.insert(metric_key.clone(), metric_value.clone());
                        }
                    }
                }
            }
            "resources" => {
                let entry = base_map.entry(key.clone()).or_insert_with(|| json!({}));
                if let (Some(dst), Some(src)) = (entry.as_object_mut(), value.as_object()) {
                    for (resource_key, resource_value) in src {
                        if !resource_value.is_null()
                            && dst.get(resource_key).is_none_or(Value::is_null)
                        {
                            dst.insert(resource_key.clone(), resource_value.clone());
                        }
                    }
                }
            }
            _ => {
                let missing = base_map
                    .get(key)
                    .map(|existing| {
                        existing.is_null()
                            || existing.as_str() == Some("")
                            || existing.as_array().is_some_and(Vec::is_empty)
                            || existing.as_object().is_some_and(serde_json::Map::is_empty)
                    })
                    .unwrap_or(true);
                if missing && !value.is_null() {
                    base_map.insert(key.clone(), value.clone());
                }
            }
        }
    }
    base
}

fn papers_from_response(data: &Value) -> Vec<Value> {
    if let Some(array) = data.as_array() {
        return array.iter().map(normalize_paper).collect();
    }
    ["papers", "trendingPapers", "results"]
        .iter()
        .find_map(|key| data.get(key).and_then(Value::as_array))
        .map(|array| array.iter().map(normalize_paper).collect())
        .unwrap_or_default()
}

fn dedupe_papers(papers: Vec<Value>) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for paper in papers {
        let key = first_present(&[
            paper.get("canonical_id"),
            paper.get("alphaxiv_id"),
            paper.get("title"),
        ])
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
        if key.as_ref().is_some_and(|k| !seen.insert(k.clone())) {
            continue;
        }
        out.push(paper);
    }
    out
}

fn metric_i64(paper: &Value, key: &str) -> i64 {
    paper
        .get("metrics")
        .and_then(|metrics| metrics.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn paper_metric_score(paper: &Value) -> i64 {
    metric_i64(paper, "public_total_votes") * 100
        + metric_i64(paper, "github_stars") * 25
        + metric_i64(paper, "visits_7d") * 2
        + metric_i64(paper, "visits_all") / 10
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::trim)
        .filter(|term| term.len() >= 3)
        .map(|term| term.to_ascii_lowercase())
        .collect()
}

fn paper_search_text(paper: &Value) -> String {
    let mut text = String::new();
    for key in ["title", "summary"] {
        if let Some(value) = paper.get(key).and_then(Value::as_str) {
            text.push(' ');
            text.push_str(value);
        }
    }
    if let Some(topics) = paper.get("topics").and_then(Value::as_array) {
        for topic in topics.iter().filter_map(Value::as_str) {
            text.push(' ');
            text.push_str(topic);
        }
    }
    text.to_ascii_lowercase()
}

fn matched_query_terms(paper: &Value, terms: &[String]) -> Vec<String> {
    let text = paper_search_text(paper);
    terms
        .iter()
        .filter(|term| text.contains(term.as_str()))
        .cloned()
        .collect()
}

fn reading_lane(paper: &Value) -> &'static str {
    let score = paper_metric_score(paper);
    let likes = metric_i64(paper, "public_total_votes");
    let stars = metric_i64(paper, "github_stars");
    let visits = metric_i64(paper, "visits_7d").max(metric_i64(paper, "visits_all"));
    if likes >= 5 || stars >= 5 || score >= 500 {
        "read_now"
    } else if likes >= 1 || stars >= 1 || visits >= 100 || score >= 100 {
        "skim"
    } else {
        "watch"
    }
}

fn compact_date(paper: &Value, key: &str) -> Option<String> {
    paper
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.split('T').next())
        .map(ToOwned::to_owned)
}

fn triage_reasons(paper: &Value, terms: &[String]) -> Vec<String> {
    let mut reasons = Vec::new();
    let matches = matched_query_terms(paper, terms);
    if !matches.is_empty() {
        reasons.push(format!("query match: {}", matches.join(", ")));
    }
    let likes = metric_i64(paper, "public_total_votes");
    let stars = metric_i64(paper, "github_stars");
    let visits = metric_i64(paper, "visits_7d");
    if likes > 0 {
        reasons.push(format!("{likes} alphaXiv likes"));
    }
    if stars > 0 {
        reasons.push(format!("{stars} GitHub stars"));
    }
    if visits > 0 {
        reasons.push(format!("{visits} visits in 7d"));
    }
    if let Some(date) = compact_date(paper, "first_seen_at") {
        reasons.push(format!("first seen {date}"));
    }
    reasons
}

fn next_reading_commands(paper: &Value) -> Vec<String> {
    let Some(id) = paper.get("alphaxiv_id").and_then(Value::as_str) else {
        return Vec::new();
    };
    let mut commands = vec![
        format!(
            "zcli alphaxiv markdown {} --kind abs --max-chars 6000 --format text",
            shell_quote(id)
        ),
        format!("zcli alphaxiv overview {} --format json", shell_quote(id)),
    ];
    if let Some(command) = paper
        .pointer("/zotero_plan/dry_run_commands/0")
        .and_then(Value::as_str)
    {
        commands.push(command.to_string());
    }
    commands
}

fn add_triage(papers: Vec<Value>, query: &str) -> Vec<Value> {
    let terms = query_terms(query);
    papers
        .into_iter()
        .map(|mut paper| {
            let triage = json!({
                "lane": reading_lane(&paper),
                "score": paper_metric_score(&paper),
                "matched_terms": matched_query_terms(&paper, &terms),
                "reasons": triage_reasons(&paper, &terms),
                "next_commands": next_reading_commands(&paper),
            });
            if let Some(object) = paper.as_object_mut() {
                object.insert("triage".to_string(), triage);
            }
            paper
        })
        .collect()
}

fn paper_matches_topics(paper: &Value, topics: &[String]) -> bool {
    if topics.is_empty() {
        return true;
    }
    let Some(paper_topics) = paper.get("topics").and_then(Value::as_array) else {
        return false;
    };
    topics.iter().all(|needle| {
        let needle = needle.to_ascii_lowercase();
        paper_topics.iter().any(|topic| {
            topic
                .as_str()
                .map(|topic| topic.to_ascii_lowercase().contains(&needle))
                .unwrap_or(false)
        })
    })
}

fn paper_date_values(paper: &Value, field: DateField) -> Vec<DateTime<Utc>> {
    let keys: &[&str] = match field {
        DateField::FirstSeen => &["first_seen_at"],
        DateField::Published => &["published_at"],
        DateField::Updated => &["updated_at"],
        DateField::Any => &["first_seen_at", "published_at", "updated_at"],
    };
    keys.iter()
        .filter_map(|key| {
            paper
                .get(*key)
                .and_then(Value::as_str)
                .and_then(parse_datetime)
        })
        .collect()
}

fn paper_matches_since(paper: &Value, cutoff: Option<DateTime<Utc>>, field: DateField) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    let dates = paper_date_values(paper, field);
    !dates.is_empty() && dates.into_iter().any(|date| date >= cutoff)
}

fn filter_rank_papers(
    mut papers: Vec<Value>,
    topics: &[String],
    min_likes: Option<i64>,
    min_github_stars: Option<i64>,
    min_visits: Option<i64>,
    cutoff: Option<DateTime<Utc>>,
    date_field: DateField,
    rank_metrics: bool,
) -> Vec<Value> {
    papers.retain(|paper| {
        paper_matches_topics(paper, topics)
            && paper_matches_since(paper, cutoff, date_field)
            && min_likes.is_none_or(|min| metric_i64(paper, "public_total_votes") >= min)
            && min_github_stars.is_none_or(|min| metric_i64(paper, "github_stars") >= min)
            && min_visits.is_none_or(|min| {
                metric_i64(paper, "visits_all").max(metric_i64(paper, "visits_7d")) >= min
            })
    });
    if rank_metrics {
        papers.sort_by_key(|paper| std::cmp::Reverse(paper_metric_score(paper)));
    }
    papers
}

fn paper_zotero_plan_from_normalized(paper: &Value) -> Value {
    let mut plan = import_plan::alphaxiv_zotero_plan(paper);
    if let Some(object) = plan.as_object_mut() {
        object.insert("note_markdown".to_string(), json!(metrics_note(paper)));
    }
    plan
}

fn attach_zotero_plans(papers: Vec<Value>) -> Vec<Value> {
    papers
        .into_iter()
        .map(|mut paper| {
            let plan = paper_zotero_plan_from_normalized(&paper);
            if let Some(object) = paper.as_object_mut() {
                object.insert("zotero_plan".to_string(), plan);
            }
            paper
        })
        .collect()
}

fn feed(args: &FeedArgs) -> Result<Value> {
    let client = client(args.timeout);
    let cutoff = since_cutoff(&args.since, args.days)?;
    let mut papers = Vec::new();
    let mut page_num = 0usize;
    let mut page_size = args.page_size;
    while papers.len() < args.limit {
        let mut last_error = None;
        let mut batch = Vec::new();
        let mut actual_size = page_size;
        for size in [page_size, 100, 50, 20] {
            if size > page_size {
                continue;
            }
            let data = request_json(
                &client,
                "/papers/v3/feed",
                &[
                    ("pageNum", page_num.to_string()),
                    ("pageSize", size.to_string()),
                    ("sort", args.sort.as_str().to_string()),
                    ("interval", args.interval.clone()),
                    ("topics", "[]".to_string()),
                ],
                matches!(args.sort, FeedSort::Recommended),
            );
            match data {
                Ok(data) => {
                    batch = papers_from_response(&data);
                    actual_size = size;
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        if batch.is_empty() {
            if let Some(error) = last_error {
                return Err(error);
            }
            break;
        }
        page_size = actual_size;
        let batch_len = batch.len();
        papers.extend(batch);
        papers = dedupe_papers(papers);
        if batch_len < actual_size {
            break;
        }
        page_num += 1;
    }
    let papers = filter_rank_papers(
        papers,
        &args.topics,
        args.min_likes,
        args.min_github_stars,
        args.min_visits,
        cutoff,
        args.date_field,
        args.rank_metrics,
    );
    let papers = papers.into_iter().take(args.limit).collect::<Vec<_>>();
    let papers = if args.with_zotero_plan {
        attach_zotero_plans(papers)
    } else {
        papers
    };
    Ok(json!({
        "source": "alphaxiv",
        "query": {
            "sort": args.sort.as_str(),
            "interval": args.interval,
            "limit": args.limit,
            "page_size": page_size,
            "topics": args.topics,
            "since": cutoff.map(|dt| dt.to_rfc3339()),
            "date_field": args.date_field.as_str(),
            "rank_metrics": args.rank_metrics,
        },
        "count": papers.len(),
        "papers": papers,
    }))
}

fn search(args: &SearchArgs) -> Result<Value> {
    let client = client(args.timeout);
    let cutoff = since_cutoff(&args.since, args.days)?;
    let mut errors = Vec::new();
    let (mode, data) = if args.fast {
        (
            "fast",
            request_json(
                &client,
                "/search/v2/paper/fast",
                &[
                    ("q", args.query.clone()),
                    ("includePrivate", "false".to_string()),
                ],
                false,
            )?,
        )
    } else {
        match request_json(
            &client,
            "/v1/search/paper",
            &[("q", args.query.clone())],
            false,
        ) {
            Ok(data) => ("full", data),
            Err(error) => {
                errors.push(json!({"endpoint": "full_search", "message": error.to_string()}));
                (
                    "fast_fallback",
                    request_json(
                        &client,
                        "/search/v2/paper/fast",
                        &[
                            ("q", args.query.clone()),
                            ("includePrivate", "false".to_string()),
                        ],
                        false,
                    )?,
                )
            }
        }
    };
    let papers = filter_rank_papers(
        papers_from_response(&data),
        &args.topics,
        args.min_likes,
        args.min_github_stars,
        args.min_visits,
        cutoff,
        args.date_field,
        args.rank_metrics,
    );
    let papers = papers.into_iter().take(args.limit).collect::<Vec<_>>();
    let papers = if args.with_zotero_plan {
        attach_zotero_plans(papers)
    } else {
        papers
    };
    Ok(json!({
        "source": "alphaxiv",
        "query": args.query,
        "search": mode,
        "since": cutoff.map(|dt| dt.to_rfc3339()),
        "date_field": args.date_field.as_str(),
        "count": papers.len(),
        "papers": papers,
        "errors": errors,
    }))
}

fn run_discovery_pipeline(
    client: &Agent,
    pipeline: DiscoveryPipeline<'_>,
) -> Result<(Vec<Value>, Vec<Value>)> {
    let mut errors = Vec::new();
    let search_data = match request_json(
        client,
        "/v1/search/paper",
        &[("q", pipeline.query.to_string())],
        false,
    ) {
        Ok(data) => data,
        Err(error) => {
            errors.push(json!({"endpoint": "full_search", "message": error.to_string()}));
            request_json(
                client,
                "/search/v2/paper/fast",
                &[
                    ("q", pipeline.query.to_string()),
                    ("includePrivate", "false".to_string()),
                ],
                false,
            )?
        }
    };
    let mut papers = papers_from_response(&search_data);
    if papers.len() < pipeline.limit {
        match request_json(
            client,
            "/papers/v3/feed",
            &[
                ("pageNum", "0".to_string()),
                ("pageSize", "50".to_string()),
                ("sort", pipeline.fallback_sort.as_str().to_string()),
                ("interval", pipeline.fallback_interval.to_string()),
                ("topics", "[]".to_string()),
            ],
            false,
        ) {
            Ok(data) => papers.extend(papers_from_response(&data)),
            Err(error) => {
                errors.push(json!({"endpoint": "fallback_feed", "message": error.to_string()}))
            }
        }
    }
    let papers = dedupe_papers(papers);
    let papers = filter_rank_papers(
        papers,
        pipeline.topics,
        pipeline.min_likes,
        pipeline.min_github_stars,
        pipeline.min_visits,
        pipeline.cutoff,
        pipeline.date_field,
        true,
    );
    let papers = attach_zotero_plans(papers.into_iter().take(pipeline.limit).collect());
    let papers = if pipeline.triage {
        add_triage(papers, pipeline.query)
    } else {
        papers
    };
    Ok((papers, errors))
}

fn discover(args: &DiscoverArgs) -> Result<Value> {
    let client = client(args.timeout);
    let cutoff = since_cutoff(&args.since, args.days)?;
    let since = cutoff.as_ref().map(DateTime::to_rfc3339);
    let (papers, errors) = run_discovery_pipeline(
        &client,
        DiscoveryPipeline {
            query: &args.query,
            limit: args.limit,
            fallback_sort: args.fallback_sort,
            fallback_interval: &args.fallback_interval,
            topics: &args.topics,
            min_likes: args.min_likes,
            min_github_stars: args.min_github_stars,
            min_visits: args.min_visits,
            cutoff,
            date_field: args.date_field,
            triage: false,
        },
    )?;
    Ok(json!({
        "source": "alphaxiv",
        "mode": "discover",
        "query": args.query,
        "since": since,
        "date_field": args.date_field.as_str(),
        "count": papers.len(),
        "papers": papers,
        "errors": errors,
        "selection_hint": "Ranked by alphaXiv metrics after query/topic filters; Zotero commands are dry-run only.",
    }))
}

fn brief(args: &BriefArgs) -> Result<Value> {
    let client = client(args.timeout);
    let cutoff = since_cutoff(&args.since, args.days)?;
    let since = cutoff.as_ref().map(DateTime::to_rfc3339);
    let (papers, errors) = run_discovery_pipeline(
        &client,
        DiscoveryPipeline {
            query: &args.query,
            limit: args.limit,
            fallback_sort: args.fallback_sort,
            fallback_interval: &args.fallback_interval,
            topics: &args.topics,
            min_likes: args.min_likes,
            min_github_stars: args.min_github_stars,
            min_visits: args.min_visits,
            cutoff,
            date_field: args.date_field,
            triage: true,
        },
    )?;
    let read_now = papers
        .iter()
        .filter(|paper| paper.pointer("/triage/lane").and_then(Value::as_str) == Some("read_now"))
        .count();
    let skim = papers
        .iter()
        .filter(|paper| paper.pointer("/triage/lane").and_then(Value::as_str) == Some("skim"))
        .count();
    let watch = papers
        .iter()
        .filter(|paper| paper.pointer("/triage/lane").and_then(Value::as_str) == Some("watch"))
        .count();
    Ok(json!({
        "source": "alphaxiv",
        "mode": "brief",
        "query": args.query,
        "time_window": {
            "since": since,
            "date_field": args.date_field.as_str(),
            "days": args.days,
        },
        "triage_counts": {
            "read_now": read_now,
            "skim": skim,
            "watch": watch,
        },
        "count": papers.len(),
        "papers": papers,
        "errors": errors,
        "selection_hint": "Triage is deterministic: query-term overlap, alphaXiv metrics, GitHub stars, visits, and the selected time field. All Zotero actions are dry-run commands.",
    }))
}

fn fetch_paper_bundle(client: &Agent, paper_id: &str, include_preview: bool) -> Value {
    let mut compact = Value::Null;
    let mut legacy = Value::Null;
    let mut preview = Value::Null;
    let mut errors = Vec::new();
    for (label, path) in [
        ("compact", format!("/papers/v3/{paper_id}")),
        ("legacy", format!("/papers/v3/legacy/{paper_id}")),
    ] {
        match request_json(client, &path, &[], false) {
            Ok(data) if label == "compact" => compact = data,
            Ok(data) => legacy = data,
            Err(error) => errors.push(json!({"endpoint": label, "message": error.to_string()})),
        }
    }
    if include_preview {
        match request_json(
            client,
            &format!("/papers/v3/{paper_id}/preview"),
            &[],
            false,
        ) {
            Ok(data) => preview = data,
            Err(error) => errors.push(json!({"endpoint": "preview", "message": error.to_string()})),
        }
    }
    let mut normalized = if !compact.is_null() {
        normalize_paper(&compact)
    } else {
        json!({})
    };
    if !legacy.is_null() {
        normalized = merge_normalized(normalized, normalize_paper(&legacy));
    }
    if !preview.is_null() {
        normalized = merge_normalized(normalized, normalize_paper(&preview));
    }
    json!({
        "source": "alphaxiv",
        "id": paper_id,
        "normalized": normalized,
        "compact": compact,
        "legacy": legacy,
        "preview": preview,
        "errors": errors,
    })
}

fn paper(args: &PaperArgs) -> Result<Value> {
    let client = client(args.timeout);
    let paper_id = paper_id_from_input(&args.paper_id);
    let bundle = fetch_paper_bundle(&client, &paper_id, !args.no_preview);
    if args.raw {
        Ok(bundle)
    } else {
        Ok(json!({
            "source": "alphaxiv",
            "id": paper_id,
            "paper": bundle.get("normalized").cloned().unwrap_or(Value::Null),
            "errors": bundle.get("errors").cloned().unwrap_or(json!([])),
        }))
    }
}

fn resolve_version_id(client: &Agent, paper_id: &str) -> Result<String> {
    let data = request_json(client, &format!("/papers/v3/{paper_id}"), &[], false)?;
    string_value(first_present(&[
        data.get("versionId"),
        data.get("version_id"),
    ]))
    .ok_or_else(|| anyhow!("No versionId found for {paper_id}"))
}

fn resolve_canonical_id(client: &Agent, paper_id: &str) -> Result<String> {
    if is_versioned_id(paper_id) {
        return Ok(paper_id.to_string());
    }
    let bundle = fetch_paper_bundle(client, paper_id, false);
    bundle
        .pointer("/normalized/canonical_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("No canonical/versioned ID found for {paper_id}"))
}

fn overview(args: &OverviewArgs) -> Result<Value> {
    let client = client(args.timeout);
    let paper_id = paper_id_from_input(&args.paper_id);
    let version_id = if is_uuid(&paper_id) {
        paper_id.clone()
    } else {
        resolve_version_id(&client, &paper_id)?
    };
    let suffix = if args.status { "status" } else { &args.lang };
    let data = request_json(
        &client,
        &format!("/papers/v3/{version_id}/overview/{suffix}"),
        &[],
        false,
    )?;
    Ok(json!({"source": "alphaxiv", "id": paper_id, "version_id": version_id, "overview": data}))
}

fn markdown(args: &MarkdownArgs) -> Result<Value> {
    let client = client(args.timeout);
    let paper_id = paper_id_from_input(&args.paper_id);
    let route = match args.kind {
        MarkdownKind::Abs => "abs",
        MarkdownKind::Overview => "overview",
    };
    let url = format!("{WEB_BASE}/{route}/{paper_id}.md");
    let text = request_text(&client, &url)?;
    if let Some(path) = &args.output {
        fs::write(path, &text).with_context(|| format!("failed to write {}", path.display()))?;
    }
    let shown = if args.max_chars > 0 {
        text.chars().take(args.max_chars).collect::<String>()
    } else {
        text.clone()
    };
    if args.format == "text" {
        return Ok(json!({"markdown": shown}));
    }
    Ok(json!({
        "source": "alphaxiv",
        "id": paper_id,
        "kind": route,
        "url": url,
        "chars": text.chars().count(),
        "truncated": args.max_chars > 0 && text.chars().count() > args.max_chars,
        "output": args.output,
        "markdown": shown,
    }))
}

fn pdf(args: &PdfArgs) -> Result<Value> {
    let client = client(args.timeout);
    let paper_id = paper_id_from_input(&args.paper_id);
    let canonical_id = resolve_canonical_id(&client, &paper_id)?;
    let url = format!("{PDF_BASE}/{canonical_id}");
    let mut result = json!({
        "source": "alphaxiv",
        "id": paper_id,
        "canonical_id": canonical_id,
        "pdf_url": url,
    });
    if let Some(path) = &args.download {
        let bytes = request_bytes(&client, result["pdf_url"].as_str().unwrap())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &bytes).with_context(|| format!("failed to write {}", path.display()))?;
        result["download"] = json!(path);
        result["bytes"] = json!(bytes.len());
    }
    Ok(result)
}

fn metrics_note(paper: &Value) -> String {
    let metrics = paper.get("metrics").unwrap_or(&Value::Null);
    format!(
        "## alphaXiv metrics\n\n- alphaXiv URL: {}\n- Overview: {}\n- Public likes: {}\n- Internal votes: {}\n- X likes: {}\n- GitHub stars: {}\n- Visits: {}\n- Retrieved: {}",
        paper.get("url").and_then(Value::as_str).unwrap_or(""),
        paper.get("overview_url").and_then(Value::as_str).unwrap_or(""),
        metrics.get("public_total_votes").unwrap_or(&Value::Null),
        metrics.get("total_votes").unwrap_or(&Value::Null),
        metrics.get("x_likes").unwrap_or(&Value::Null),
        metrics.get("github_stars").unwrap_or(&Value::Null),
        metrics.get("visits_all").unwrap_or(&Value::Null),
        Local::now().date_naive(),
    )
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

fn zotero_plan(args: &ZoteroPlanArgs) -> Result<Value> {
    let client = client(args.timeout);
    let paper_id = paper_id_from_input(&args.paper_id);
    let bundle = fetch_paper_bundle(&client, &paper_id, false);
    let paper = bundle.get("normalized").cloned().unwrap_or(Value::Null);
    let plan = paper_zotero_plan_from_normalized(&paper);
    Ok(json!({
        "source": "alphaxiv",
        "id": paper_id,
        "paper": paper,
        "import_strategy": plan.get("import_strategy").cloned().unwrap_or(Value::Null),
        "dry_run_commands": plan.get("dry_run_commands").cloned().unwrap_or(json!([])),
        "note_markdown": plan.get("note_markdown").cloned().unwrap_or(Value::Null),
        "write_note_command_template": "zcli write note ITEMKEY --content '<note_markdown>' --dry-run --format json",
        "mutation_policy": "Do not add --execute unless the user explicitly approves the Zotero mutation in the current turn.",
        "errors": bundle.get("errors").cloned().unwrap_or(json!([])),
    }))
}

fn auth_status(args: &AuthStatusArgs) -> Result<Value> {
    let client = client(args.timeout);
    let data = clerk_client(&client)?;
    let response = data.get("response").unwrap_or(&Value::Null);
    let session = active_clerk_session(&data);
    let jwt = session
        .pointer("/last_active_token/jwt")
        .and_then(Value::as_str)
        .unwrap_or("");
    let payload = jwt_payload(jwt);
    Ok(json!({
        "source": "alphaxiv",
        "auth": {
            "configured": true,
            "clerk_cookie_file": display_path(&default_cookie_file()),
            "session_status": session.get("status").cloned().unwrap_or(Value::Null),
            "session_id_hint": session.get("id").and_then(Value::as_str).map(|s| s.chars().take(8).collect::<String>()),
            "session_expires_at": iso_from_ms(session.get("expire_at").and_then(Value::as_i64)),
            "session_abandon_at": iso_from_ms(session.get("abandon_at").and_then(Value::as_i64)),
            "client_cookie_expires_at": iso_from_ms(response.get("cookie_expires_at").and_then(Value::as_i64)),
            "short_token_available": !jwt.is_empty(),
            "short_token_issued_at": iso_from_s(payload.get("iat").and_then(Value::as_i64)),
            "short_token_expires_at": iso_from_s(payload.get("exp").and_then(Value::as_i64)),
        }
    }))
}

fn auth_refresh(args: &AuthRefreshArgs) -> Result<Value> {
    let script = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agents/skills/alphaxiv/scripts/alphaxiv_auth_refresh.mjs");
    if !script.exists() {
        bail!("missing auth refresh helper: {}", script.display());
    }
    let mut command = Command::new("node");
    command
        .arg(script)
        .arg("--output")
        .arg(default_cookie_file())
        .arg("--limit")
        .arg(args.limit.to_string())
        .arg("--interval")
        .arg(&args.interval);
    if args.no_open {
        command.arg("--no-open");
    }
    if args.no_validate_feed {
        command.arg("--no-validate-feed");
    }
    let output = command
        .output()
        .context("failed to run alphaXiv auth-refresh helper")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        serde_json::from_str(stdout.trim()).context("auth-refresh helper returned non-JSON output")
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{}",
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_helpers_cover_arxiv_and_alphaxiv_ids() {
        assert_eq!(arxiv_base_id("2604.25850"), Some("2604.25850".to_string()));
        assert_eq!(
            arxiv_base_id("2604.25850v3"),
            Some("2604.25850".to_string())
        );
        assert_eq!(arxiv_base_id("visual-primitivesv1"), None);
        assert!(is_versioned_id("visual-primitivesv1"));
        assert!(is_uuid("019de1ab-7e12-7fa7-b8e2-1cd59cf7ba4e"));
        assert!(!is_uuid("2604.25850"));
    }

    #[test]
    fn paper_id_input_accepts_common_urls() {
        assert_eq!(
            paper_id_from_input("https://www.alphaxiv.org/abs/2604.25850?sort=Hot"),
            "2604.25850"
        );
        assert_eq!(
            paper_id_from_input("https://www.alphaxiv.org/overview/visual-primitives.md"),
            "visual-primitives"
        );
        assert_eq!(
            paper_id_from_input("https://arxiv.org/pdf/2604.25850v3.pdf"),
            "2604.25850v3"
        );
        assert_eq!(
            paper_id_from_input("https://fetcher.alphaxiv.org/v2/pdf/visual-primitivesv1"),
            "visual-primitivesv1"
        );
    }

    #[test]
    fn normalizes_flat_feed_paper() {
        let paper = normalize_paper(&json!({
            "id": "019dd735-b8bd-7ab5-abcc-1ebf0973ec8f",
            "universal_paper_id": "2604.25850",
            "canonical_id": "2604.25850v3",
            "versionId": "019de1ab-7e12-7fa7-b8e2-1cd59cf7ba4e",
            "title": "Agentic Harness Engineering",
            "resources": {"github": {"url": "https://github.com/example/ahe", "stars": 7}},
            "metrics": {
                "public_total_votes": 74,
                "total_votes": 13,
                "visits_count": {"all": 870, "last_7_days": 12}
            }
        }));
        assert_eq!(paper["alphaxiv_id"], "2604.25850");
        assert_eq!(paper["canonical_id"], "2604.25850v3");
        assert_eq!(paper["group_id"], "019dd735-b8bd-7ab5-abcc-1ebf0973ec8f");
        assert_eq!(
            paper["resources"]["github"],
            "https://github.com/example/ahe"
        );
        assert_eq!(paper["metrics"]["github_stars"], 7);
        assert_eq!(paper["first_seen_at"], Value::Null);
        assert_eq!(
            paper["pdf_url"],
            "https://fetcher.alphaxiv.org/v2/pdf/2604.25850v3"
        );
    }

    #[test]
    fn normalizes_and_filters_dates() {
        let paper = normalize_paper(&json!({
            "universal_paper_id": "2605.00809",
            "canonical_id": "2605.00809v1",
            "first_publication_date": "Tue May 05 2026 16:55:02 GMT+0000 (Coordinated Universal Time)",
            "publication_date": "2026-05-05T17:51:38.000Z",
            "updated_at": "2026-05-06T02:00:17.735Z",
            "metrics": {"public_total_votes": 1, "visits_count": {"all": 3}}
        }));
        assert_eq!(paper["first_seen_at"], "2026-05-05T16:55:02+00:00");
        let cutoff = parse_datetime("2026-05-05").unwrap();
        assert!(paper_matches_since(
            &paper,
            Some(cutoff),
            DateField::FirstSeen
        ));
        let later = parse_datetime("2026-05-06").unwrap();
        assert!(!paper_matches_since(
            &paper,
            Some(later),
            DateField::Published
        ));
        assert!(paper_matches_since(&paper, Some(later), DateField::Updated));
    }

    #[test]
    fn merge_normalized_preserves_existing_and_fills_missing_metrics() {
        let merged = merge_normalized(
            json!({
                "title": "Base",
                "metrics": {"github_stars": null, "visits_all": 10},
                "resources": {}
            }),
            json!({
                "title": "Extra",
                "metrics": {"github_stars": 3, "visits_all": 99},
                "resources": {"github": "https://github.com/example/repo"}
            }),
        );
        assert_eq!(merged["title"], "Base");
        assert_eq!(merged["metrics"]["github_stars"], 3);
        assert_eq!(merged["metrics"]["visits_all"], 10);
        assert_eq!(
            merged["resources"]["github"],
            "https://github.com/example/repo"
        );
    }
}
