use std::{collections::HashMap, fs, path::PathBuf, process::Command, time::Duration};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, Utc};
use clap::ValueEnum;
use regex::Regex;
use serde_json::{json, Value};
use ureq::Agent;

use crate::{
    alphaxiv::{self, DateField, FeedSort},
    config::Config,
    zotero::ZoteroDb,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InboxSource {
    Alphaxiv,
    Huggingface,
    X,
}

impl InboxSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Alphaxiv => "alphaxiv",
            Self::Huggingface => "huggingface",
            Self::X => "x",
        }
    }
}

pub struct FetchOptions<'a> {
    pub config: &'a Config,
    pub source: InboxSource,
    pub query: Option<&'a str>,
    pub handles: &'a [String],
    pub configured_x_handles: &'a [String],
    pub tweet_limit: usize,
    pub limit: usize,
    pub fallback_sort: FeedSort,
    pub fallback_interval: &'a str,
    pub topics: &'a [String],
    pub min_likes: Option<i64>,
    pub min_github_stars: Option<i64>,
    pub min_visits: Option<i64>,
    pub since: Option<&'a str>,
    pub days: Option<i64>,
    pub date_field: DateField,
    pub timeout: u64,
    pub context: bool,
    pub code_overview: bool,
    pub show_seen: bool,
    pub show_existing: bool,
    pub seen_days: i64,
    pub cache_overview: bool,
    pub overview_ttl_days: i64,
    pub dry_run: bool,
    pub execute: bool,
}

pub struct DiscussionOptions<'a> {
    pub paper: &'a str,
    pub handles: &'a [String],
    pub tweets: &'a [String],
    pub days: i64,
    pub search_limit: usize,
    pub limit: usize,
    pub reply_limit: usize,
    pub max_pages: usize,
    pub expand: bool,
}

pub fn status(config: &Config) -> Value {
    json!({
        "ok": true,
        "status": "ready",
        "dry_run_first": true,
        "candidate_schema": "paper_candidate/v1",
        "sources": [
            {
                "name": "alphaxiv",
                "status": "ready",
                "capabilities": [
                    "search",
                    "feed_fallback",
                    "time_window",
                    "time_semantics",
                    "local_context_match",
                    "metric_ranking",
                    "triage",
                    "workflow_plan",
                    "quick_code_overview",
                    "zotero_dry_run_plan"
                ],
                "time_fields": ["first_seen", "published", "updated", "any"]
            },
            {
                "name": "huggingface",
                "status": "ready",
                "capabilities": [
                    "daily_papers",
                    "paper_search",
                    "time_window",
                    "time_semantics",
                    "local_context_match",
                    "github_signal",
                    "workflow_plan",
                    "quick_code_overview",
                    "zotero_dry_run_plan"
                ],
                "time_fields": ["first_seen", "published", "updated", "any"]
            },
            {
                "name": "x",
                "status": "ready",
                "capabilities": [
                    "bird_user_tweets",
                    "bird_search",
                    "paper_link_extraction",
                    "paper_discussion_search",
                    "reply_question_mining",
                    "time_window",
                    "time_semantics",
                    "local_context_match",
                    "social_signal",
                    "workflow_plan",
                    "quick_code_overview",
                    "zotero_dry_run_plan"
                ],
                "time_fields": ["first_seen", "published", "updated", "any"],
                "configured_handles": config.inbox.x_handles,
            }
        ],
    })
}

pub fn x_handles(config: &Config) -> Value {
    json!({
        "ok": true,
        "source": "x",
        "handles": config.inbox.x_handles,
        "count": config.inbox.x_handles.len(),
    })
}

pub fn add_x_handle(config: &mut Config, handle: &str) -> Result<Value> {
    let handle = normalize_x_handle(handle)?;
    if !config.inbox.x_handles.iter().any(|value| value == &handle) {
        config.inbox.x_handles.push(handle.clone());
        config.inbox.x_handles.sort();
    }
    Ok(json!({
        "ok": true,
        "source": "x",
        "action": "add",
        "handle": handle,
        "handles": config.inbox.x_handles,
    }))
}

pub fn remove_x_handle(config: &mut Config, handle: &str) -> Result<Value> {
    let handle = normalize_x_handle(handle)?;
    config.inbox.x_handles.retain(|value| value != &handle);
    Ok(json!({
        "ok": true,
        "source": "x",
        "action": "remove",
        "handle": handle,
        "handles": config.inbox.x_handles,
    }))
}

pub fn fetch(options: FetchOptions<'_>) -> Result<Value> {
    let query = options.query.unwrap_or("").trim();
    let source = match options.source {
        InboxSource::Alphaxiv => {
            if matches!(options.fallback_sort, FeedSort::Recommended)
                && !(options.config.risk.high_risk_auth_enabled
                    && options.config.risk.alphaxiv_auth_enabled)
            {
                return Err(anyhow!(
                    "alphaXiv Recommended uses the local Clerk session; enable high-risk alphaXiv auth in `zcli setup` before fetching it through inbox"
                ));
            }
            if query.is_empty() {
                alphaxiv::feed(&alphaxiv::FeedArgs {
                    sort: options.fallback_sort,
                    interval: options.fallback_interval.to_string(),
                    limit: options.limit,
                    page_size: options.limit.clamp(20, 100),
                    topics: options.topics.to_vec(),
                    min_likes: options.min_likes,
                    min_github_stars: options.min_github_stars,
                    min_visits: options.min_visits,
                    rank_metrics: false,
                    with_zotero_plan: true,
                    since: options.since.map(ToOwned::to_owned),
                    days: options.days,
                    date_field: options.date_field,
                    timeout: options.timeout,
                })?
            } else {
                alphaxiv::discover_with_options(alphaxiv::DiscoveryOptions {
                    query,
                    limit: options.limit,
                    fallback_sort: options.fallback_sort,
                    fallback_interval: options.fallback_interval,
                    topics: options.topics,
                    min_likes: options.min_likes,
                    min_github_stars: options.min_github_stars,
                    min_visits: options.min_visits,
                    since: options.since,
                    days: options.days,
                    date_field: options.date_field,
                    timeout: options.timeout,
                    triage: true,
                })?
            }
        }
        InboxSource::Huggingface => fetch_huggingface(&options, query)?,
        InboxSource::X => fetch_x(&options, query)?,
    };
    let local_context = if options.context {
        local_context_profile(options.config)
    } else {
        ContextProfile::default()
    };
    let papers = source
        .get("papers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut candidates = papers
        .iter()
        .enumerate()
        .map(|(index, paper)| paper_candidate(index + 1, options.source, query, paper))
        .collect::<Vec<_>>();
    ensure_candidate_triage(&mut candidates, options.source, query);
    apply_context_profile(&mut candidates, &local_context);
    if options.code_overview {
        attach_code_overviews(&mut candidates, options.timeout);
    }
    let duplicate_report = apply_inbox_filters(&mut candidates, &options).unwrap_or_else(|error| {
        json!({
            "ok": false,
            "message": error.to_string(),
        })
    });
    if options.cache_overview && matches!(options.source, InboxSource::Alphaxiv) {
        attach_alphaxiv_overview_cache(
            options.config,
            &mut candidates,
            options.timeout,
            options.overview_ttl_days,
        );
    }
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate["rank"] = json!(index + 1);
    }
    let state_update = if options.execute {
        mark_candidates_seen(options.config, &candidates)
    } else {
        Ok(json!({"executed": false, "reason": "pass --execute to record displayed candidates as seen"}))
    }
    .unwrap_or_else(|error| json!({"executed": false, "error": error.to_string()}));

    Ok(json!({
        "ok": true,
        "dry_run": options.dry_run,
        "executed": options.execute,
        "schema": "paper_candidate/v1",
        "source": options.source.as_str(),
        "query": query,
        "time_window": {
            "since": source.get("since").cloned().unwrap_or(Value::Null),
            "days": options.days,
            "date_field": options.date_field.as_str(),
            "semantics": time_semantics(options.source, options.date_field),
        },
        "context_profile": local_context.to_json(),
        "count": candidates.len(),
        "dedupe": duplicate_report,
        "state_update": state_update,
        "candidates": candidates,
        "source_errors": source.get("errors").cloned().unwrap_or_else(|| json!([])),
        "next_step": "Read cached alphaXiv overview/markdown first. Run a returned Zotero dry-run command only after selecting papers to import.",
    }))
}

pub fn discussion(options: DiscussionOptions<'_>) -> Result<Value> {
    let paper = paper_query_profile(options.paper);
    let cutoff = since_cutoff(None, Some(options.days))?;
    let mut tweets = Vec::new();
    let mut errors = Vec::new();

    for tweet in options.tweets {
        match bird_read(tweet) {
            Ok(data) => tweets.extend(tweet_values(&data)),
            Err(error) => errors.push(json!({
                "endpoint": "bird_read",
                "target": tweet,
                "message": error.to_string(),
            })),
        }
    }

    let searched_queries = discussion_search_queries(&paper, options.handles, cutoff.as_ref());
    for query in &searched_queries {
        match bird_search(&query, None, options.search_limit) {
            Ok(data) => tweets.extend(tweet_values(&data)),
            Err(error) => errors.push(json!({
                "endpoint": "bird_search",
                "query": query,
                "message": error.to_string(),
            })),
        }
    }

    let mut posts = dedupe_tweets(tweets)
        .into_iter()
        .filter(|tweet| tweet_matches_paper(tweet, &paper))
        .map(|tweet| discussion_post(&tweet, &paper, options.handles))
        .collect::<Vec<_>>();
    posts.sort_by_key(|post| std::cmp::Reverse(post_score(post)));
    posts.truncate(options.limit);

    let mut discussion_items = Vec::new();
    if options.expand {
        for post in &posts {
            if let Some(target) = post.get("url").and_then(Value::as_str) {
                match bird_replies(target, options.max_pages) {
                    Ok(data) => discussion_items.extend(discussion_items_from_tweets(
                        tweet_values(&data),
                        post,
                        "reply",
                    )),
                    Err(error) => errors.push(json!({
                        "endpoint": "bird_replies",
                        "target": target,
                        "message": error.to_string(),
                    })),
                }
                match bird_thread(target, options.max_pages) {
                    Ok(data) => discussion_items.extend(discussion_items_from_tweets(
                        tweet_values(&data),
                        post,
                        "thread",
                    )),
                    Err(error) => errors.push(json!({
                        "endpoint": "bird_thread",
                        "target": target,
                        "message": error.to_string(),
                    })),
                }
            }
        }
    }
    let root_ids = posts
        .iter()
        .filter_map(|post| post.get("tweet_id").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>();
    discussion_items = dedupe_discussion_items(discussion_items)
        .into_iter()
        .filter(|item| {
            item.get("tweet_id")
                .and_then(Value::as_str)
                .is_none_or(|id| !root_ids.contains(id))
        })
        .collect();
    discussion_items.sort_by_key(|item| std::cmp::Reverse(discussion_item_score(item)));
    discussion_items.truncate(options.reply_limit);

    Ok(json!({
        "ok": true,
        "schema": "paper_discussion/v1",
        "source": "x",
        "paper": paper.to_json(),
        "time_window": {
            "days": options.days,
            "since": cutoff.map(|date| date.to_rfc3339()),
            "semantics": "X post/reply creation time; paper publication time is only inferred from paper identifiers",
        },
        "search": {
            "queries": searched_queries,
            "search_limit": options.search_limit,
            "expanded_replies_and_threads": options.expand,
            "max_pages": options.max_pages,
        },
        "announcement_count": posts.len(),
        "discussion_count": discussion_items.len(),
        "announcement_posts": posts,
        "discussion_items": discussion_items,
        "source_errors": errors,
        "notes": [
            "Read-only Bird/X scan.",
            "Quote repost coverage is best-effort through search; replies/thread expansion is bounded by --max-pages.",
            "Value labels are heuristic and favor questions, author answers, limitations, reproduction, benchmark, code, and dataset discussion."
        ],
    }))
}

#[derive(Debug)]
struct PaperQuery {
    raw: String,
    title: Option<String>,
    arxiv_id: Option<String>,
    urls: Vec<String>,
    keywords: Vec<String>,
}

impl PaperQuery {
    fn to_json(&self) -> Value {
        json!({
            "raw": self.raw,
            "title": self.title,
            "arxiv_id": self.arxiv_id,
            "urls": self.urls,
            "keywords": self.keywords,
        })
    }
}

fn paper_query_profile(input: &str) -> PaperQuery {
    let raw = input.trim().to_string();
    let arxiv_id = arxiv_id_from_text(&raw);
    let urls = paper_links(&raw);
    let title = if raw.starts_with("http://")
        || raw.starts_with("https://")
        || arxiv_id.as_deref() == Some(raw.as_str())
    {
        None
    } else {
        Some(raw.clone())
    };
    let mut keywords = Vec::new();
    if let Some(title) = &title {
        collect_terms_from_text(title, &mut keywords);
    }
    if let Some(id) = &arxiv_id {
        keywords.push(id.clone());
    }
    for url in &urls {
        if let Some(id) = arxiv_id_from_text(url) {
            keywords.push(id);
        }
        if let Some(last) = url.rsplit('/').next().filter(|value| value.len() >= 4) {
            keywords.push(last.trim_end_matches(".pdf").to_string());
        }
    }
    keywords.sort();
    keywords.dedup();
    keywords.truncate(12);
    PaperQuery {
        raw,
        title,
        arxiv_id,
        urls,
        keywords,
    }
}

fn discussion_search_queries(
    paper: &PaperQuery,
    handles: &[String],
    cutoff: Option<&DateTime<Utc>>,
) -> Vec<String> {
    let mut bases = Vec::new();
    if let Some(title) = &paper.title {
        bases.push(format!("\"{}\"", title.replace('"', "")));
        let compact = paper
            .keywords
            .iter()
            .filter(|term| term.len() >= 5)
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if !compact.is_empty() {
            bases.push(compact);
        }
    }
    if let Some(id) = &paper.arxiv_id {
        bases.push(id.clone());
        bases.push(format!("arxiv.org/abs/{id}"));
    }
    for url in &paper.urls {
        bases.push(url.clone());
    }
    if bases.is_empty() {
        bases.push(paper.raw.clone());
    }

    let since = cutoff.map(|date| format!(" since:{}", date.date_naive()));
    let mut queries = Vec::new();
    for base in bases {
        let dated = format!("{}{}", base, since.as_deref().unwrap_or(""));
        queries.push(dated.clone());
        for handle in handles {
            if let Ok(handle) = normalize_x_handle(handle) {
                queries.push(format!("from:{handle} {dated}"));
            }
        }
    }
    queries.sort();
    queries.dedup();
    queries.truncate(12);
    queries
}

fn tweet_matches_paper(tweet: &Value, paper: &PaperQuery) -> bool {
    let text = all_strings(tweet).join(" ").to_ascii_lowercase();
    if paper
        .arxiv_id
        .as_ref()
        .is_some_and(|id| text.contains(&id.to_ascii_lowercase()))
    {
        return true;
    }
    if paper
        .urls
        .iter()
        .any(|url| text.contains(&url.to_ascii_lowercase()))
    {
        return true;
    }
    let matched_terms = paper
        .keywords
        .iter()
        .filter(|term| term.len() >= 4 && text.contains(&term.to_ascii_lowercase()))
        .count();
    matched_terms >= 2
        || paper.title.as_ref().is_some_and(|title| {
            let title = title.to_ascii_lowercase();
            title.len() >= 16 && text.contains(&title)
        })
}

fn discussion_post(tweet: &Value, paper: &PaperQuery, handles: &[String]) -> Value {
    let author = tweet_author(tweet);
    let tweet_id = tweet_id(tweet);
    let url = tweet_url(tweet, &author, &tweet_id);
    let text = tweet_text(tweet);
    let normalized_handles = handles
        .iter()
        .filter_map(|handle| normalize_x_handle(handle).ok())
        .collect::<Vec<_>>();
    let author_priority = normalized_handles
        .iter()
        .any(|handle| handle.eq_ignore_ascii_case(&author));
    json!({
        "tweet_id": tweet_id,
        "url": url,
        "author": if author.is_empty() { Value::Null } else { json!(format!("@{author}")) },
        "created_at": normalize_datetime_value(first_existing(tweet, &["createdAt", "created_at", "date"])),
        "text": text,
        "role": if author_priority { "prioritized_author_or_curator" } else if tweet_matches_paper(tweet, paper) { "matched_announcement_or_discussion_seed" } else { "unknown" },
        "signals": tweet_signals(tweet),
        "matched": {
            "arxiv_id": paper.arxiv_id,
            "keywords": matched_keywords(&text, &paper.keywords),
        },
    })
}

fn discussion_items_from_tweets(tweets: Vec<Value>, root: &Value, source: &str) -> Vec<Value> {
    let root_author = root
        .get("author")
        .and_then(Value::as_str)
        .map(strip_at)
        .unwrap_or_default();
    let root_id = root
        .get("tweet_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    tweets
        .iter()
        .filter_map(|tweet| {
            let text = tweet_text(tweet);
            if text.trim().is_empty() || !is_valuable_discussion_text(&text) {
                return None;
            }
            let author = tweet_author(tweet);
            let id = tweet_id(tweet);
            let labels = discussion_labels(&text, author.eq_ignore_ascii_case(&root_author));
            Some(json!({
                "tweet_id": id,
                "url": tweet_url(tweet, &author, &id),
                "source": source,
                "root_tweet_id": root_id,
                "root_url": root.get("url").cloned().unwrap_or(Value::Null),
                "author": if author.is_empty() { Value::Null } else { json!(format!("@{author}")) },
                "created_at": normalize_datetime_value(first_existing(tweet, &["createdAt", "created_at", "date"])),
                "text": text,
                "value_labels": labels,
                "signals": tweet_signals(tweet),
                "reply_to": first_existing(tweet, &["inReplyToStatusId", "in_reply_to_status_id", "parentId"]).cloned().unwrap_or(Value::Null),
            }))
        })
        .collect()
}

fn is_valuable_discussion_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    text.contains('?')
        || [
            "why",
            "how",
            "what",
            "does",
            "can",
            "could",
            "limitation",
            "baseline",
            "benchmark",
            "ablation",
            "reproduce",
            "reproduction",
            "dataset",
            "code",
            "github",
            "license",
            "eval",
            "metric",
            "latency",
            "memory",
            "cost",
            "failure",
            "compare",
            "vs ",
            "answer",
            "we found",
            "we use",
            "we did",
            "because",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn discussion_labels(text: &str, same_as_root_author: bool) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut labels = Vec::new();
    if text.contains('?')
        || ["why", "how", "what", "does", "can", "could"]
            .iter()
            .any(|v| lower.contains(v))
    {
        labels.push("question".to_string());
    }
    let answer_cue = [
        "we found", "we use", "we did", "because", "answer", "we are",
    ]
    .iter()
    .any(|v| lower.contains(v));
    if answer_cue || (same_as_root_author && !text.contains('?')) {
        labels.push("possible_author_answer".to_string());
    }
    if ["limitation", "failure", "edge case", "doesn't", "not work"]
        .iter()
        .any(|v| lower.contains(v))
    {
        labels.push("limitation_or_failure".to_string());
    }
    if [
        "baseline",
        "benchmark",
        "ablation",
        "eval",
        "metric",
        "compare",
        " vs ",
    ]
    .iter()
    .any(|v| lower.contains(v))
    {
        labels.push("benchmark_or_comparison".to_string());
    }
    if [
        "reproduce",
        "reproduction",
        "code",
        "github",
        "dataset",
        "license",
    ]
    .iter()
    .any(|v| lower.contains(v))
    {
        labels.push("implementation_or_data".to_string());
    }
    if labels.is_empty() {
        labels.push("technical_discussion".to_string());
    }
    labels
}

fn post_score(post: &Value) -> i64 {
    let role_boost =
        if post.get("role").and_then(Value::as_str) == Some("prioritized_author_or_curator") {
            1000
        } else {
            0
        };
    role_boost
        + post
            .pointer("/signals/likes")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 10
        + post
            .pointer("/signals/reposts")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 20
        + post
            .pointer("/signals/replies")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 30
}

fn discussion_item_score(item: &Value) -> i64 {
    let labels = item
        .get("value_labels")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let label_boost = labels
        .iter()
        .filter_map(Value::as_str)
        .map(|label| match label {
            "possible_author_answer" => 500,
            "question" => 200,
            "limitation_or_failure" => 180,
            "benchmark_or_comparison" => 160,
            "implementation_or_data" => 140,
            _ => 50,
        })
        .sum::<i64>();
    label_boost
        + item
            .pointer("/signals/likes")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 5
        + item
            .pointer("/signals/reposts")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 10
}

fn dedupe_tweets(tweets: Vec<Value>) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tweet in tweets {
        let id = tweet_id(&tweet);
        let key = if id.is_empty() {
            tweet_text(&tweet)
        } else {
            id
        };
        if !key.is_empty() && seen.insert(key) {
            out.push(tweet);
        }
    }
    out
}

fn dedupe_discussion_items(items: Vec<Value>) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let id = item
            .get("tweet_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let key = if id.is_empty() {
            item.get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        } else {
            id
        };
        if !key.is_empty() && seen.insert(key) {
            out.push(item);
        }
    }
    out
}

fn paper_candidate(rank: usize, source: InboxSource, query: &str, paper: &Value) -> Value {
    let id = paper
        .get("alphaxiv_id")
        .or_else(|| paper.get("huggingface_id"))
        .or_else(|| paper.get("arxiv_id"))
        .or_else(|| paper.get("tweet_id"))
        .or_else(|| paper.get("canonical_id"))
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({
        "rank": rank,
        "candidate_id": format!("{}:{}", source.as_str(), id),
        "source": source.as_str(),
        "query": query,
        "title": paper.get("title").cloned().unwrap_or(Value::Null),
        "authors": paper.get("authors").cloned().unwrap_or_else(|| json!([])),
        "summary": paper.get("summary").cloned().unwrap_or(Value::Null),
        "identifiers": {
            "alphaxiv_id": paper.get("alphaxiv_id").cloned().unwrap_or(Value::Null),
            "canonical_id": paper.get("canonical_id").cloned().unwrap_or(Value::Null),
            "arxiv_id": paper.get("arxiv_id").cloned().unwrap_or(Value::Null),
            "huggingface_id": paper.get("huggingface_id").cloned().unwrap_or(Value::Null),
            "tweet_id": paper.get("tweet_id").cloned().unwrap_or(Value::Null),
        },
        "urls": {
            "source": paper.get("url").cloned().unwrap_or(Value::Null),
            "pdf": paper.get("pdf_url").cloned().unwrap_or(Value::Null),
            "overview": paper.get("overview_url").cloned().unwrap_or(Value::Null),
            "tweet": paper.get("tweet_url").cloned().unwrap_or(Value::Null),
        },
        "time": {
            "first_seen_at": paper.get("first_seen_at").cloned().unwrap_or(Value::Null),
            "published_at": paper.get("published_at").cloned().unwrap_or(Value::Null),
            "updated_at": paper.get("updated_at").cloned().unwrap_or(Value::Null),
            "semantics": {
                "first_seen_at": time_field_semantics(source, DateField::FirstSeen),
                "published_at": time_field_semantics(source, DateField::Published),
                "updated_at": time_field_semantics(source, DateField::Updated),
            },
        },
        "signals": {
            "topics": paper.get("topics").cloned().unwrap_or_else(|| json!([])),
            "metrics": paper.get("metrics").cloned().unwrap_or_else(|| json!({})),
            "resources": paper.get("resources").cloned().unwrap_or_else(|| json!({})),
        },
        "triage": paper.get("triage").cloned().unwrap_or_else(|| json!({})),
        "workflow": workflow_plan(source, paper),
        "zotero_plan": paper.get("zotero_plan").cloned().unwrap_or(Value::Null),
        "raw_source": paper,
    })
}

fn ensure_candidate_triage(candidates: &mut [Value], source: InboxSource, query: &str) {
    for candidate in candidates {
        if candidate
            .pointer("/triage/lane")
            .and_then(Value::as_str)
            .is_some()
        {
            continue;
        }
        let score = candidate_signal_score(candidate);
        let lane = if score >= 500 {
            "read_now"
        } else if score > 0 {
            "skim"
        } else {
            "watch"
        };
        let mut reasons = Vec::new();
        if !query.is_empty() {
            reasons.push(format!("{} source match", source.as_str()));
        }
        if let Some(date) = candidate
            .pointer("/time/first_seen_at")
            .and_then(Value::as_str)
            .or_else(|| {
                candidate
                    .pointer("/time/published_at")
                    .and_then(Value::as_str)
            })
            .and_then(|value| value.split('T').next())
        {
            reasons.push(format!("source date {date}"));
        }
        let metrics = candidate
            .pointer("/signals/metrics")
            .unwrap_or(&Value::Null);
        for (label, key) in [
            ("alphaXiv likes", "public_total_votes"),
            ("GitHub stars", "github_stars"),
            ("visits", "visits_7d"),
            ("HF upvotes", "upvotes"),
            ("X likes", "likes"),
        ] {
            if let Some(value) = metrics
                .get(key)
                .and_then(Value::as_i64)
                .filter(|value| *value > 0)
            {
                reasons.push(format!("{value} {label}"));
            }
        }
        candidate["triage"] = json!({
            "lane": lane,
            "score": score,
            "reasons": reasons,
            "next_commands": candidate
                .pointer("/workflow/before_import/0/command")
                .and_then(Value::as_str)
                .map(|command| vec![command.to_string()])
                .unwrap_or_default(),
        });
    }
}

fn candidate_signal_score(candidate: &Value) -> i64 {
    let metrics = candidate
        .pointer("/signals/metrics")
        .unwrap_or(&Value::Null);
    metrics
        .get("public_total_votes")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        * 100
        + metrics
            .get("github_stars")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 25
        + metrics
            .get("visits_7d")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 2
        + metrics
            .get("visits_all")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            / 10
        + metrics.get("upvotes").and_then(Value::as_i64).unwrap_or(0) * 100
        + metrics.get("likes").and_then(Value::as_i64).unwrap_or(0) * 10
        + metrics.get("retweets").and_then(Value::as_i64).unwrap_or(0) * 20
}

fn state_dir(config: &Config) -> Result<PathBuf> {
    config
        .state_dir
        .clone()
        .map(|path| path.join("inbox"))
        .ok_or_else(|| anyhow!("zcli state_dir is unavailable; cannot read inbox seen state"))
}

fn seen_state_path(config: &Config) -> Result<PathBuf> {
    Ok(state_dir(config)?.join("seen.json"))
}

fn candidate_key(candidate: &Value) -> String {
    candidate
        .get("candidate_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn read_seen_entries(config: &Config) -> Result<HashMap<String, Value>> {
    let path = seen_state_path(config)?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read inbox seen state {}", path.display()))?;
    let values = serde_json::from_str::<Vec<Value>>(&raw)
        .with_context(|| format!("failed to parse inbox seen state {}", path.display()))?;
    Ok(values
        .into_iter()
        .filter_map(|entry| {
            let key = entry
                .get("candidate_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)?;
            Some((key, entry))
        })
        .collect())
}

fn seen_at(entry: &Value) -> Option<DateTime<Utc>> {
    entry
        .get("last_seen_at")
        .and_then(Value::as_str)
        .and_then(parse_datetime)
}

fn seen_is_recent_without_spike(candidate: &Value, entry: &Value, seen_days: i64) -> bool {
    if seen_days <= 0 {
        return false;
    }
    let Some(last_seen) = seen_at(entry) else {
        return false;
    };
    if last_seen < Utc::now() - ChronoDuration::days(seen_days) {
        return false;
    }
    let previous_score = entry.get("score").and_then(Value::as_i64).unwrap_or(0);
    let current_score = candidate_signal_score(candidate);
    current_score <= previous_score.saturating_mul(2).saturating_add(100)
}

fn candidate_duplicate_queries(candidate: &Value) -> Vec<(String, i64)> {
    let mut queries = Vec::new();
    for path in [
        "/identifiers/arxiv_id",
        "/identifiers/alphaxiv_id",
        "/identifiers/canonical_id",
    ] {
        if let Some(value) = candidate.pointer(path).and_then(Value::as_str) {
            if let Some(id) = arxiv_id_from_text(value) {
                queries.push((id, 94));
            }
        }
    }
    if let Some(url) = candidate.pointer("/urls/source").and_then(Value::as_str) {
        queries.push((url.to_string(), 90));
    }
    if let Some(title) = candidate.get("title").and_then(Value::as_str) {
        if title.chars().count() >= 12 {
            queries.push((title.to_string(), 70));
        }
    }
    queries.sort_by(|a, b| a.0.cmp(&b.0));
    queries.dedup_by(|a, b| a.0 == b.0);
    queries
}

fn existing_matches_for_candidate(db: &ZoteroDb, candidate: &Value) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut matches = Vec::new();
    for (query, threshold) in candidate_duplicate_queries(candidate) {
        for item in db.resolve_items(&query, 3).unwrap_or_default() {
            let score = item.get("score").and_then(Value::as_i64).unwrap_or(0);
            if score < threshold {
                continue;
            }
            let key = item
                .pointer("/item/key")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if key.is_empty() || seen.insert(key) {
                matches.push(item);
            }
        }
    }
    matches
}

fn apply_inbox_filters(candidates: &mut Vec<Value>, options: &FetchOptions<'_>) -> Result<Value> {
    let seen_entries = if options.show_seen {
        HashMap::new()
    } else {
        read_seen_entries(options.config).unwrap_or_default()
    };
    let db_result = if options.show_existing {
        None
    } else {
        Some(ZoteroDb::open(options.config))
    };
    let mut existing_unavailable = None;
    let db = match db_result {
        Some(Ok(db)) => Some(db),
        Some(Err(error)) => {
            existing_unavailable = Some(error.to_string());
            None
        }
        None => None,
    };

    let mut hidden_existing = 0usize;
    let mut hidden_seen = 0usize;
    let mut surfaced_spikes = 0usize;
    let mut examples = Vec::new();

    candidates.retain_mut(|candidate| {
        if let Some(db) = db.as_ref() {
            let matches = existing_matches_for_candidate(db, candidate);
            if !matches.is_empty() {
                candidate["existing_in_library"] = json!({
                    "status": "matched",
                    "matches": matches,
                });
                if !options.show_existing {
                    hidden_existing += 1;
                    if examples.len() < 5 {
                        examples.push(json!({
                            "reason": "existing_in_library",
                            "candidate_id": candidate.get("candidate_id").cloned().unwrap_or(Value::Null),
                            "title": candidate.get("title").cloned().unwrap_or(Value::Null),
                        }));
                    }
                    return false;
                }
            } else {
                candidate["existing_in_library"] = json!({"status": "not_found"});
            }
        } else {
            candidate["existing_in_library"] = json!({
                "status": "unavailable",
                "reason": existing_unavailable,
            });
        }

        let key = candidate_key(candidate);
        if !options.show_seen {
            if let Some(entry) = seen_entries.get(&key) {
                if seen_is_recent_without_spike(candidate, entry, options.seen_days) {
                    hidden_seen += 1;
                    if examples.len() < 5 {
                        examples.push(json!({
                            "reason": "seen_recently",
                            "candidate_id": key,
                            "last_seen_at": entry.get("last_seen_at").cloned().unwrap_or(Value::Null),
                            "title": candidate.get("title").cloned().unwrap_or(Value::Null),
                        }));
                    }
                    return false;
                }
                surfaced_spikes += 1;
                candidate["seen_state"] = json!({
                    "status": "resurfaced",
                    "reason": "metrics_spike_or_seen_window_expired",
                    "previous_score": entry.get("score").cloned().unwrap_or(Value::Null),
                    "current_score": candidate_signal_score(candidate),
                    "last_seen_at": entry.get("last_seen_at").cloned().unwrap_or(Value::Null),
                });
            }
        }
        true
    });

    Ok(json!({
        "show_seen": options.show_seen,
        "show_existing": options.show_existing,
        "seen_days": options.seen_days,
        "hidden_existing": hidden_existing,
        "hidden_seen_recently": hidden_seen,
        "surfaced_metric_spikes_or_expired": surfaced_spikes,
        "existing_check": if existing_unavailable.is_some() { "unavailable" } else if options.show_existing { "disabled_by_flag" } else { "local_zotero_db" },
        "examples": examples,
    }))
}

fn attach_alphaxiv_overview_cache(
    config: &Config,
    candidates: &mut [Value],
    timeout: u64,
    ttl_days: i64,
) {
    for candidate in candidates {
        let Some(id) = candidate
            .pointer("/identifiers/alphaxiv_id")
            .and_then(Value::as_str)
        else {
            continue;
        };
        let value = alphaxiv::cache_overview_markdown(config, id, timeout, ttl_days)
            .unwrap_or_else(|error| {
                json!({
                    "kind": "overview_markdown",
                    "ok": false,
                    "error": error.to_string(),
                })
            });
        if let Some(object) = candidate.as_object_mut() {
            let cache = object
                .entry("cache".to_string())
                .or_insert_with(|| json!({}));
            if let Some(cache_object) = cache.as_object_mut() {
                cache_object.insert("overview_markdown".to_string(), value);
            }
        }
    }
}

fn mark_candidates_seen(config: &Config, candidates: &[Value]) -> Result<Value> {
    let path = seen_state_path(config)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut entries = read_seen_entries(config).unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    for candidate in candidates {
        let key = candidate_key(candidate);
        if key.is_empty() {
            continue;
        }
        entries.insert(
            key.clone(),
            json!({
                "candidate_id": key,
                "source": candidate.get("source").cloned().unwrap_or(Value::Null),
                "title": candidate.get("title").cloned().unwrap_or(Value::Null),
                "last_seen_at": now,
                "score": candidate_signal_score(candidate),
                "identifiers": candidate.get("identifiers").cloned().unwrap_or(Value::Null),
                "urls": candidate.get("urls").cloned().unwrap_or(Value::Null),
            }),
        );
    }
    let mut values = entries.into_values().collect::<Vec<_>>();
    values.sort_by(|a, b| {
        b.get("last_seen_at")
            .and_then(Value::as_str)
            .cmp(&a.get("last_seen_at").and_then(Value::as_str))
    });
    values.truncate(2_000);
    fs::write(&path, serde_json::to_vec_pretty(&values)?)
        .with_context(|| format!("failed to write inbox seen state {}", path.display()))?;
    Ok(json!({
        "executed": true,
        "kind": "mark_seen",
        "path": path,
        "count": candidates.len(),
    }))
}

#[derive(Default)]
struct ContextProfile {
    enabled: bool,
    terms: Vec<String>,
    recent_titles: Vec<String>,
    queue_titles: Vec<String>,
    unavailable_reason: Option<String>,
}

impl ContextProfile {
    fn to_json(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "terms": self.terms,
            "recent_titles": self.recent_titles,
            "queue_titles": self.queue_titles,
            "unavailable_reason": self.unavailable_reason,
        })
    }
}

fn local_context_profile(config: &Config) -> ContextProfile {
    let mut profile = ContextProfile {
        enabled: true,
        ..ContextProfile::default()
    };
    match ZoteroDb::open(config) {
        Ok(db) => match db.recent(60, 12) {
            Ok(items) => {
                for item in items {
                    if let Some(title) = item.title {
                        profile.recent_titles.push(title.clone());
                        collect_terms_from_text(&title, &mut profile.terms);
                    }
                    for author in item.authors {
                        collect_terms_from_text(&author, &mut profile.terms);
                    }
                }
            }
            Err(error) => profile.unavailable_reason = Some(error.to_string()),
        },
        Err(error) => profile.unavailable_reason = Some(error.to_string()),
    }
    if let Some(state_dir) = &config.state_dir {
        let path = state_dir.join("queue.json");
        if let Ok(raw) = fs::read_to_string(path) {
            if let Ok(items) = serde_json::from_str::<Vec<Value>>(&raw) {
                for item in items {
                    for key in ["title", "note"] {
                        if let Some(text) = item.get(key).and_then(Value::as_str) {
                            if key == "title" {
                                profile.queue_titles.push(text.to_string());
                            }
                            collect_terms_from_text(text, &mut profile.terms);
                        }
                    }
                }
            }
        }
    }
    profile.terms.sort();
    profile.terms.dedup();
    profile.terms.truncate(24);
    profile.recent_titles.truncate(8);
    profile.queue_titles.truncate(8);
    profile
}

fn collect_terms_from_text(text: &str, out: &mut Vec<String>) {
    const STOPWORDS: &[&str] = &[
        "about", "after", "agent", "agents", "paper", "using", "with", "from", "into", "that",
        "this", "their", "model", "models", "learning", "based", "towards", "toward", "large",
        "language", "study", "system", "systems",
    ];
    for term in text
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|term| term.trim().to_ascii_lowercase())
        .filter(|term| term.len() >= 4 && !STOPWORDS.contains(&term.as_str()))
    {
        out.push(term);
    }
}

fn apply_context_profile(candidates: &mut [Value], profile: &ContextProfile) {
    if profile.terms.is_empty() {
        for candidate in candidates {
            candidate["context_match"] = json!({"score": 0, "matched_terms": []});
        }
        return;
    }
    for candidate in candidates.iter_mut() {
        let text = all_strings(candidate).join(" ").to_ascii_lowercase();
        let matched = profile
            .terms
            .iter()
            .filter(|term| text.contains(term.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let score = matched.len() as i64 * 25;
        candidate["context_match"] = json!({
            "score": score,
            "matched_terms": matched,
            "basis": "recent Zotero items and reading queue",
        });
        if score > 0 {
            let reasons = candidate
                .pointer_mut("/triage/reasons")
                .and_then(Value::as_array_mut);
            if let Some(reasons) = reasons {
                reasons.push(json!("matches local Zotero/queue context"));
            }
        }
    }
    candidates.sort_by_key(|candidate| {
        let context_score = candidate
            .pointer("/context_match/score")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let triage_score = candidate
            .pointer("/triage/score")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        std::cmp::Reverse(context_score * 10 + triage_score)
    });
}

fn workflow_plan(source: InboxSource, paper: &Value) -> Value {
    let import_command = paper
        .pointer("/zotero_plan/dry_run_commands/0")
        .and_then(Value::as_str);
    let mut before_import = Vec::new();
    if let Some(command) = import_command {
        before_import.push(json!({
            "label": "preview_import",
            "command": command,
        }));
    }
    let source_label = source.as_str();
    json!({
        "dry_run_first": true,
        "before_import": before_import,
        "after_import_with_item_key": [
            {
                "label": "add_to_reading_queue",
                "command": "zcli queue add <ITEMKEY> --note \"from inbox candidate\"",
            },
            {
                "label": "tag_source",
                "command": format!("zcli write tags <ITEMKEY> --add inbox --add source:{source_label} --dry-run --format json"),
            },
            {
                "label": "create_reading_context",
                "command": "zcli context <ITEMKEY> --budget 40k --format json",
            }
        ],
    })
}

fn time_semantics(source: InboxSource, field: DateField) -> Value {
    let selected = match field {
        DateField::FirstSeen => time_field_semantics(source, DateField::FirstSeen),
        DateField::Published => time_field_semantics(source, DateField::Published),
        DateField::Updated => time_field_semantics(source, DateField::Updated),
        DateField::Any => "matches any available first_seen_at, published_at, or updated_at for the selected source",
    };
    json!({
        "selected": selected,
        "first_seen": time_field_semantics(source, DateField::FirstSeen),
        "published": time_field_semantics(source, DateField::Published),
        "updated": time_field_semantics(source, DateField::Updated),
    })
}

fn time_field_semantics(source: InboxSource, field: DateField) -> &'static str {
    match (source, field) {
        (InboxSource::Alphaxiv, DateField::FirstSeen) => "first seen on alphaXiv",
        (InboxSource::Alphaxiv, DateField::Published) => {
            "paper publication date from alphaXiv/arXiv metadata"
        }
        (InboxSource::Alphaxiv, DateField::Updated) => {
            "paper update date from alphaXiv/arXiv metadata"
        }
        (InboxSource::Huggingface, DateField::FirstSeen) => {
            "submitted to Hugging Face daily papers"
        }
        (InboxSource::Huggingface, DateField::Published) => {
            "paper publication date from Hugging Face paper metadata"
        }
        (InboxSource::Huggingface, DateField::Updated) => {
            "Hugging Face daily submission time or publication date"
        }
        (InboxSource::X, DateField::FirstSeen) => "post creation time from X/Bird",
        (InboxSource::X, DateField::Published) => {
            "not usually available from X; use first_seen/any unless an extracted paper date exists"
        }
        (InboxSource::X, DateField::Updated) => {
            "post creation time from X/Bird; X posts do not expose paper update time"
        }
        (_, DateField::Any) => "any available source time",
    }
}

fn attach_code_overviews(candidates: &mut [Value], timeout: u64) {
    let client = http_client(timeout);
    for candidate in candidates {
        let github = candidate
            .pointer("/signals/resources/github")
            .and_then(Value::as_str)
            .or_else(|| {
                candidate
                    .pointer("/raw_source/resources/github")
                    .and_then(Value::as_str)
            });
        let overview = github
            .and_then(github_repo_slug)
            .map(|slug| {
                github_quick_overview(&client, &slug).unwrap_or_else(|error| {
                    json!({
                        "ok": false,
                        "repo": slug,
                        "error": error.to_string(),
                        "depth": "quick_api_metadata",
                    })
                })
            })
            .unwrap_or_else(|| json!({"ok": false, "reason": "no GitHub repository link"}));
        candidate["code_overview"] = overview;
    }
}

fn github_repo_slug(url: &str) -> Option<String> {
    let re = Regex::new(r"github\.com[:/]([^/\s]+)/([^/\s#?]+)").expect("valid GitHub regex");
    let captures = re.captures(url)?;
    let owner = captures.get(1)?.as_str();
    let repo = captures
        .get(2)?
        .as_str()
        .trim_end_matches(".git")
        .trim_end_matches('/');
    Some(format!("{owner}/{repo}"))
}

fn github_quick_overview(client: &Agent, slug: &str) -> Result<Value> {
    let mut response = client
        .get(format!("https://api.github.com/repos/{slug}"))
        .header("User-Agent", "zotero-cli")
        .header("Accept", "application/vnd.github+json")
        .header("Accept-Encoding", "identity")
        .call()
        .context("GitHub quick overview request failed")?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .context("failed to read GitHub response")?;
    if !status.is_success() {
        return Err(anyhow!(
            "GitHub HTTP {}: {}",
            status.as_u16(),
            concise_http_error(&text)
        ));
    }
    let data: Value = serde_json::from_str(&text).context("GitHub returned non-JSON response")?;
    Ok(json!({
        "ok": true,
        "depth": "quick_api_metadata",
        "repo": slug,
        "url": data.get("html_url").cloned().unwrap_or(Value::Null),
        "description": data.get("description").cloned().unwrap_or(Value::Null),
        "stars": data.get("stargazers_count").cloned().unwrap_or(Value::Null),
        "forks": data.get("forks_count").cloned().unwrap_or(Value::Null),
        "open_issues": data.get("open_issues_count").cloned().unwrap_or(Value::Null),
        "language": data.get("language").cloned().unwrap_or(Value::Null),
        "default_branch": data.get("default_branch").cloned().unwrap_or(Value::Null),
        "pushed_at": data.get("pushed_at").cloned().unwrap_or(Value::Null),
        "archived": data.get("archived").cloned().unwrap_or(Value::Null),
    }))
}

fn concise_http_error(text: &str) -> String {
    let message = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| text.trim().chars().take(180).collect::<String>());
    if message.to_ascii_lowercase().contains("rate limit") {
        "GitHub API rate limit exceeded".to_string()
    } else {
        message
    }
}

fn fetch_huggingface(options: &FetchOptions<'_>, query: &str) -> Result<Value> {
    let cutoff = since_cutoff(options.since, options.days)?;
    let client = http_client(options.timeout);
    let mut papers = Vec::new();
    let mut errors = Vec::new();
    if query.is_empty() {
        for date in hf_dates(cutoff, options.days) {
            match request_hf_json(
                &client,
                "https://huggingface.co/api/daily_papers",
                &[("date", date)],
            ) {
                Ok(data) => papers.extend(hf_papers_from_daily(&data)),
                Err(error) => {
                    errors.push(json!({"endpoint": "daily_papers", "message": error.to_string()}))
                }
            }
        }
    } else {
        match request_hf_json(
            &client,
            "https://huggingface.co/api/papers/search",
            &[("q", query.to_string())],
        ) {
            Ok(data) => papers.extend(hf_papers_from_search(&data)),
            Err(error) => {
                errors.push(json!({"endpoint": "paper_search", "message": error.to_string()}))
            }
        }
        if papers.len() < options.limit {
            for date in hf_dates(cutoff, options.days) {
                match request_hf_json(
                    &client,
                    "https://huggingface.co/api/daily_papers",
                    &[("date", date)],
                ) {
                    Ok(data) => papers.extend(hf_papers_from_daily(&data)),
                    Err(error) => errors
                        .push(json!({"endpoint": "daily_papers", "message": error.to_string()})),
                }
            }
        }
    }
    let mut papers = dedupe_by_id(papers);
    papers.retain(|paper| {
        paper_matches_query(paper, query)
            && paper_matches_since(paper, cutoff, options.date_field)
            && options
                .min_likes
                .is_none_or(|min| metric_i64(paper, "upvotes") >= min)
            && options
                .min_github_stars
                .is_none_or(|min| metric_i64(paper, "github_stars") >= min)
    });
    papers.sort_by_key(|paper| {
        std::cmp::Reverse(metric_i64(paper, "upvotes") * 100 + metric_i64(paper, "github_stars"))
    });
    let papers = add_basic_triage(
        papers.into_iter().take(options.limit).collect(),
        query,
        "Hugging Face",
    );
    Ok(json!({
        "source": "huggingface",
        "query": query,
        "since": cutoff.map(|date| date.to_rfc3339()),
        "date_field": options.date_field.as_str(),
        "count": papers.len(),
        "papers": papers,
        "errors": errors,
    }))
}

fn fetch_x(options: &FetchOptions<'_>, query: &str) -> Result<Value> {
    let handles = if options.handles.is_empty() {
        options.configured_x_handles
    } else {
        options.handles
    };
    if query.is_empty() && handles.is_empty() {
        return Err(anyhow!(
            "inbox fetch --source x requires a query, at least one --handle, or configured handles from `zcli inbox sources x add HANDLE`"
        ));
    }
    let cutoff = since_cutoff(options.since, options.days)?;
    let mut tweets = Vec::new();
    let mut errors = Vec::new();
    if handles.is_empty() {
        match bird_search(query, cutoff, options.tweet_limit) {
            Ok(data) => tweets.extend(tweet_values(&data)),
            Err(error) => {
                errors.push(json!({"endpoint": "bird_search", "message": error.to_string()}))
            }
        }
    } else {
        for handle in handles {
            match bird_user_tweets(handle, options.tweet_limit) {
                Ok(data) => tweets.extend(tweet_values(&data)),
                Err(error) => errors.push(json!({"endpoint": "bird_user_tweets", "handle": handle, "message": error.to_string()})),
            }
        }
    }
    let mut papers = tweets
        .iter()
        .flat_map(|tweet| x_papers_from_tweet(tweet, query))
        .collect::<Vec<_>>();
    papers = dedupe_by_id(papers);
    papers.retain(|paper| paper_matches_since(paper, cutoff, options.date_field));
    papers.sort_by_key(|paper| {
        std::cmp::Reverse(
            metric_i64(paper, "likes") * 10
                + metric_i64(paper, "retweets") * 20
                + metric_i64(paper, "replies") * 5,
        )
    });
    let papers = add_basic_triage(
        papers.into_iter().take(options.limit).collect(),
        query,
        "X/Bird",
    );
    Ok(json!({
        "source": "x",
        "query": query,
        "handles": handles,
        "since": cutoff.map(|date| date.to_rfc3339()),
        "date_field": options.date_field.as_str(),
        "count": papers.len(),
        "papers": papers,
        "errors": errors,
    }))
}

fn http_client(timeout: u64) -> Agent {
    Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn request_hf_json(client: &Agent, url: &str, params: &[(&str, String)]) -> Result<Value> {
    let mut response = client
        .get(url)
        .query_pairs(params.iter().map(|(key, value)| (*key, value.as_str())))
        .header("User-Agent", "zotero-cli")
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .call()
        .context("Hugging Face request failed")?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .context("failed to read Hugging Face response")?;
    if !status.is_success() {
        return Err(anyhow!(
            "Hugging Face HTTP {}: {}",
            status.as_u16(),
            text.trim()
        ));
    }
    serde_json::from_str(&text).context("Hugging Face returned non-JSON response")
}

fn hf_dates(cutoff: Option<DateTime<Utc>>, days: Option<i64>) -> Vec<String> {
    let count = days.unwrap_or(1).clamp(1, 31);
    let today = Local::now().date_naive();
    let mut dates = Vec::new();
    for offset in 0..count {
        let date = today - ChronoDuration::days(offset);
        if cutoff.is_none_or(|cutoff| {
            date.and_hms_opt(23, 59, 59)
                .map(|date| DateTime::<Utc>::from_naive_utc_and_offset(date, Utc) >= cutoff)
                .unwrap_or(true)
        }) {
            dates.push(date.to_string());
        }
    }
    dates
}

fn hf_papers_from_daily(data: &Value) -> Vec<Value> {
    data.as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|entry| entry.get("paper").or(Some(entry)))
                .map(normalize_hf_paper)
                .collect()
        })
        .unwrap_or_default()
}

fn hf_papers_from_search(data: &Value) -> Vec<Value> {
    data.as_array()
        .map(|array| array.iter().map(normalize_hf_paper).collect())
        .unwrap_or_default()
}

fn normalize_hf_paper(paper: &Value) -> Value {
    let raw_id = string_field(paper, &["id", "_id"]).unwrap_or_default();
    let arxiv_id =
        arxiv_id_from_text(&raw_id).or_else(|| arxiv_id_from_text(&all_strings(paper).join(" ")));
    let id = if raw_id.is_empty() {
        arxiv_id.clone().unwrap_or_default()
    } else {
        raw_id
    };
    let github = string_field(paper, &["githubRepo"]);
    let project = string_field(paper, &["projectPage"]);
    json!({
        "source": "huggingface",
        "huggingface_id": id,
        "arxiv_id": arxiv_id,
        "title": string_field(paper, &["title"]),
        "authors": hf_authors(paper.get("authors")),
        "summary": string_field(paper, &["summary", "ai_summary"]),
        "url": if id.is_empty() { Value::Null } else { json!(format!("https://huggingface.co/papers/{id}")) },
        "pdf_url": if id.is_empty() { Value::Null } else { json!(format!("https://arxiv.org/pdf/{id}")) },
        "overview_url": Value::Null,
        "first_seen_at": normalize_datetime_value(paper.get("submittedOnDailyAt")),
        "published_at": normalize_datetime_value(paper.get("publishedAt")),
        "updated_at": normalize_datetime_value(paper.get("submittedOnDailyAt").or_else(|| paper.get("publishedAt"))),
        "metrics": {
            "upvotes": paper.get("upvotes").cloned().unwrap_or(Value::Null),
            "github_stars": paper.get("githubStars").cloned().unwrap_or(Value::Null),
        },
        "resources": {
            "github": github,
            "project": project,
            "discussion_id": paper.get("discussionId").cloned().unwrap_or(Value::Null),
            "keywords": paper.get("ai_keywords").cloned().unwrap_or_else(|| json!([])),
        },
        "zotero_plan": zotero_plan_for_arxiv(arxiv_id_from_text(&id)),
    })
}

fn hf_authors(value: Option<&Value>) -> Value {
    let names = value
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|author| string_field(author, &["name", "fullname"]))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!(names)
}

fn bird_search(query: &str, cutoff: Option<DateTime<Utc>>, limit: usize) -> Result<Value> {
    let mut search = query.to_string();
    if let Some(cutoff) = cutoff {
        search.push_str(&format!(" since:{}", cutoff.date_naive()));
    }
    run_bird(&[
        "--quote-depth",
        "0",
        "search",
        &search,
        "-n",
        &limit.to_string(),
        "--json",
    ])
}

fn bird_user_tweets(handle: &str, limit: usize) -> Result<Value> {
    run_bird(&[
        "--quote-depth",
        "0",
        "user-tweets",
        handle,
        "-n",
        &limit.to_string(),
        "--json",
    ])
}

fn bird_read(target: &str) -> Result<Value> {
    run_bird(&["read", target, "--json"])
}

fn bird_replies(target: &str, max_pages: usize) -> Result<Value> {
    run_bird(&[
        "replies",
        target,
        "--all",
        "--max-pages",
        &max_pages.to_string(),
        "--json",
    ])
}

fn bird_thread(target: &str, max_pages: usize) -> Result<Value> {
    run_bird(&[
        "thread",
        target,
        "--all",
        "--max-pages",
        &max_pages.to_string(),
        "--json",
    ])
}

fn run_bird(args: &[&str]) -> Result<Value> {
    let output = Command::new("bird")
        .args(args)
        .output()
        .context("failed to run bird CLI")?;
    if !output.status.success() {
        return Err(anyhow!(
            "bird exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).context("bird returned non-JSON output")
}

fn tweet_values(data: &Value) -> Vec<Value> {
    if let Some(array) = data.as_array() {
        return array.clone();
    }
    if data.is_object()
        && (data.get("id").is_some()
            || data.get("id_str").is_some()
            || data.get("text").is_some()
            || data.get("tweet").is_some())
    {
        if let Some(tweet) = data.get("tweet") {
            return vec![tweet.clone()];
        }
        return vec![data.clone()];
    }
    for key in ["tweets", "results", "items", "data"] {
        if let Some(array) = data.get(key).and_then(Value::as_array) {
            return array.clone();
        }
    }
    Vec::new()
}

fn tweet_id(tweet: &Value) -> String {
    string_field(tweet, &["id", "id_str", "rest_id", "tweetId"]).unwrap_or_default()
}

fn tweet_text(tweet: &Value) -> String {
    string_field(tweet, &["text", "full_text", "content", "body"])
        .unwrap_or_else(|| all_strings(tweet).join("\n"))
        .trim()
        .to_string()
}

fn tweet_url(tweet: &Value, author: &str, tweet_id: &str) -> Value {
    if let Some(url) = string_field(tweet, &["url", "tweet_url"]) {
        return json!(url);
    }
    if !tweet_id.is_empty() && !author.is_empty() {
        return json!(format!("https://x.com/{author}/status/{tweet_id}"));
    }
    Value::Null
}

fn tweet_signals(tweet: &Value) -> Value {
    json!({
        "likes": first_existing(tweet, &["likeCount", "favorite_count", "likes"]).cloned().unwrap_or(Value::Null),
        "reposts": first_existing(tweet, &["retweetCount", "retweets", "repostCount"]).cloned().unwrap_or(Value::Null),
        "replies": first_existing(tweet, &["replyCount", "replies"]).cloned().unwrap_or(Value::Null),
        "quotes": first_existing(tweet, &["quoteCount", "quotes"]).cloned().unwrap_or(Value::Null),
    })
}

fn matched_keywords(text: &str, keywords: &[String]) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    keywords
        .iter()
        .filter(|keyword| lower.contains(&keyword.to_ascii_lowercase()))
        .cloned()
        .collect()
}

fn x_papers_from_tweet(tweet: &Value, query: &str) -> Vec<Value> {
    let strings = all_strings(tweet);
    let text = strings.join("\n");
    let links = paper_links(&text);
    links
        .into_iter()
        .map(|link| {
            let arxiv_id = arxiv_id_from_text(&link);
            let tweet_id = string_field(tweet, &["id", "id_str", "rest_id"]).unwrap_or_default();
            let author = tweet_author(tweet);
            let tweet_url = if !tweet_id.is_empty() && !author.is_empty() {
                format!("https://x.com/{author}/status/{tweet_id}")
            } else {
                string_field(tweet, &["url", "tweet_url"]).unwrap_or_default()
            };
            json!({
                "source": "x",
                "tweet_id": tweet_id,
                "arxiv_id": arxiv_id,
                "title": title_from_tweet(&text, &link).unwrap_or_else(|| link.clone()),
                "authors": if author.is_empty() { json!([]) } else { json!([format!("@{author}")]) },
                "summary": text.trim(),
                "url": link,
                "tweet_url": tweet_url,
                "pdf_url": arxiv_id.as_ref().map(|id| format!("https://arxiv.org/pdf/{id}")),
                "overview_url": Value::Null,
                "first_seen_at": normalize_datetime_value(first_existing(tweet, &["createdAt", "created_at", "date"])),
                "published_at": Value::Null,
                "updated_at": normalize_datetime_value(first_existing(tweet, &["createdAt", "created_at", "date"])),
                "metrics": {
                    "likes": first_existing(tweet, &["likeCount", "favorite_count", "likes"]).cloned().unwrap_or(Value::Null),
                    "retweets": first_existing(tweet, &["retweetCount", "retweets"]).cloned().unwrap_or(Value::Null),
                    "replies": first_existing(tweet, &["replyCount", "replies"]).cloned().unwrap_or(Value::Null),
                },
                "resources": {
                    "tweet": tweet_url,
                    "matched_query": query,
                },
                "zotero_plan": zotero_plan_for_arxiv(arxiv_id),
            })
        })
        .collect()
}

fn paper_links(text: &str) -> Vec<String> {
    let url_re = Regex::new(r#"https?://[^\s<>)\]"']+"#).expect("valid URL regex");
    let mut links = url_re
        .find_iter(text)
        .map(|m| m.as_str().trim_end_matches(['.', ',', ';']).to_string())
        .filter(|url| {
            url.contains("arxiv.org/")
                || url.contains("huggingface.co/papers/")
                || url.contains("alphaxiv.org/")
        })
        .collect::<Vec<_>>();
    if links.is_empty() {
        if let Some(id) = arxiv_id_from_text(text) {
            links.push(format!("https://arxiv.org/abs/{id}"));
        }
    }
    links.sort();
    links.dedup();
    links
}

fn title_from_tweet(text: &str, link: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.contains(link) && line.len() >= 12)
        .map(|line| line.trim_matches(['"', '\'']).to_string())
}

fn tweet_author(tweet: &Value) -> String {
    first_existing(tweet, &["username", "screen_name", "authorUsername"])
        .and_then(Value::as_str)
        .map(strip_at)
        .or_else(|| {
            tweet
                .get("author")
                .and_then(|author| string_field(author, &["username", "screen_name", "user"]))
                .map(|value| strip_at(&value))
        })
        .unwrap_or_default()
}

fn add_basic_triage(mut papers: Vec<Value>, query: &str, label: &str) -> Vec<Value> {
    for paper in &mut papers {
        let score = metric_i64(paper, "upvotes") * 100
            + metric_i64(paper, "github_stars")
            + metric_i64(paper, "likes") * 10
            + metric_i64(paper, "retweets") * 20;
        let lane = if score >= 500 {
            "read_now"
        } else if score > 0 {
            "skim"
        } else {
            "watch"
        };
        let mut reasons = Vec::new();
        if !query.is_empty() && paper_matches_query(paper, query) {
            reasons.push(format!("{label} query/source match"));
        }
        if let Some(date) =
            compact_date(paper, "first_seen_at").or_else(|| compact_date(paper, "published_at"))
        {
            reasons.push(format!("source date {date}"));
        }
        if let Some(command) = paper
            .pointer("/zotero_plan/dry_run_commands/0")
            .and_then(Value::as_str)
        {
            reasons.push("Zotero dry-run plan available".to_string());
            let triage = json!({
                "lane": lane,
                "score": score,
                "reasons": reasons,
                "next_commands": [command],
            });
            if let Some(object) = paper.as_object_mut() {
                object.insert("triage".to_string(), triage);
            }
        } else if let Some(object) = paper.as_object_mut() {
            object.insert(
                "triage".to_string(),
                json!({"lane": lane, "score": score, "reasons": reasons, "next_commands": []}),
            );
        }
    }
    papers
}

fn zotero_plan_for_arxiv(arxiv_id: Option<String>) -> Value {
    let Some(id) = arxiv_id else {
        return Value::Null;
    };
    json!({
        "import_strategy": "arxiv",
        "dry_run_commands": [format!("zcli import arxiv {id} --dry-run --format json")],
    })
}

fn since_cutoff(since: Option<&str>, days: Option<i64>) -> Result<Option<DateTime<Utc>>> {
    if let Some(raw) = since.filter(|value| !value.trim().is_empty()) {
        if let Ok(date) = DateTime::parse_from_rfc3339(raw) {
            return Ok(Some(date.with_timezone(&Utc)));
        }
        let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .with_context(|| format!("invalid --since date: {raw}"))?;
        return Ok(Some(DateTime::<Utc>::from_naive_utc_and_offset(
            date.and_hms_opt(0, 0, 0).unwrap(),
            Utc,
        )));
    }
    Ok(days.map(|days| Utc::now() - ChronoDuration::days(days)))
}

fn paper_matches_since(paper: &Value, cutoff: Option<DateTime<Utc>>, field: DateField) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    let keys: &[&str] = match field {
        DateField::FirstSeen => &["first_seen_at"],
        DateField::Published => &["published_at"],
        DateField::Updated => &["updated_at"],
        DateField::Any => &["first_seen_at", "published_at", "updated_at"],
    };
    keys.iter().any(|key| {
        paper
            .get(*key)
            .and_then(Value::as_str)
            .and_then(parse_datetime)
            .is_some_and(|date| date >= cutoff)
    })
}

fn paper_matches_query(paper: &Value, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let text = all_strings(paper).join(" ").to_ascii_lowercase();
    query
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|term| term.len() >= 3)
        .all(|term| text.contains(&term.to_ascii_lowercase()))
}

fn dedupe_by_id(papers: Vec<Value>) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for paper in papers {
        let key = string_field(
            &paper,
            &[
                "arxiv_id",
                "huggingface_id",
                "alphaxiv_id",
                "tweet_id",
                "url",
                "title",
            ],
        )
        .unwrap_or_default();
        if key.is_empty() || seen.insert(key) {
            out.push(paper);
        }
    }
    out
}

fn all_strings(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_strings(value, &mut out);
    out
}

fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(value) => out.push(value.clone()),
        Value::Array(array) => {
            for item in array {
                collect_strings(item, out);
            }
        }
        Value::Object(object) => {
            for value in object.values() {
                collect_strings(value, out);
            }
        }
        _ => {}
    }
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn first_existing<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| value.get(*key))
}

fn metric_i64(paper: &Value, key: &str) -> i64 {
    paper
        .get("metrics")
        .and_then(|metrics| metrics.get(key))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn arxiv_id_from_text(text: &str) -> Option<String> {
    let re = Regex::new(r"(?i)(?:arxiv\.org/(?:abs|pdf)/)?(\d{4}\.\d{4,5})(?:v\d+)?")
        .expect("valid arXiv regex");
    re.captures(text)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn normalize_datetime_value(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_str)
        .and_then(parse_datetime)
        .map(|date| json!(date.to_rfc3339()))
        .unwrap_or(Value::Null)
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|date| DateTime::<Utc>::from_naive_utc_and_offset(date, Utc))
        })
}

fn compact_date(paper: &Value, key: &str) -> Option<String> {
    paper
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| value.split('T').next())
        .map(ToOwned::to_owned)
}

fn strip_at(value: &str) -> String {
    value.trim().trim_start_matches('@').to_string()
}

fn normalize_x_handle(handle: &str) -> Result<String> {
    let handle = strip_at(handle);
    if handle.is_empty()
        || !handle
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(anyhow!("invalid X handle: {handle}"));
    }
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_candidate_preserves_source_time_and_zotero_plan() {
        let paper = json!({
            "alphaxiv_id": "2605.00001",
            "canonical_id": "2605.00001v1",
            "title": "Agent Memory Harnesses",
            "authors": ["A. Researcher"],
            "summary": "A test paper.",
            "url": "https://www.alphaxiv.org/abs/2605.00001",
            "pdf_url": "https://fetcher.alphaxiv.org/v2/pdf/2605.00001v1",
            "first_seen_at": "2026-05-05T00:00:00+00:00",
            "published_at": "2026-05-04T00:00:00+00:00",
            "metrics": {"public_total_votes": 3, "github_stars": 9},
            "triage": {"lane": "read_now"},
            "zotero_plan": {"dry_run_commands": ["zcli import arxiv 2605.00001 --dry-run --format json"]}
        });

        let candidate = paper_candidate(1, InboxSource::Alphaxiv, "agent memory", &paper);

        assert_eq!(candidate["candidate_id"], "alphaxiv:2605.00001");
        assert_eq!(
            candidate["time"]["published_at"],
            "2026-05-04T00:00:00+00:00"
        );
        assert_eq!(candidate["triage"]["lane"], "read_now");
        assert_eq!(
            candidate["zotero_plan"]["dry_run_commands"][0],
            "zcli import arxiv 2605.00001 --dry-run --format json"
        );
    }

    #[test]
    fn normalizes_huggingface_paper_with_import_plan() {
        let paper = json!({
            "id": "2604.04979",
            "title": "Squeez: Task-Conditioned Tool-Output Pruning for Coding Agents",
            "summary": "Coding agents consume long tool observations.",
            "publishedAt": "2026-04-04T00:00:00.000Z",
            "submittedOnDailyAt": "2026-04-08T10:00:00.000Z",
            "upvotes": 10,
            "githubRepo": "https://github.com/KRLabsOrg/squeez",
            "githubStars": 13,
            "authors": [{"name": "A. Researcher"}]
        });

        let normalized = normalize_hf_paper(&paper);

        assert_eq!(normalized["source"], "huggingface");
        assert_eq!(normalized["arxiv_id"], "2604.04979");
        assert_eq!(normalized["metrics"]["upvotes"], 10);
        assert_eq!(
            normalized["zotero_plan"]["dry_run_commands"][0],
            "zcli import arxiv 2604.04979 --dry-run --format json"
        );
    }

    #[test]
    fn candidate_includes_time_semantics_and_workflow() {
        let paper = json!({
            "huggingface_id": "2604.04979",
            "arxiv_id": "2604.04979",
            "title": "Squeez",
            "published_at": "2026-04-04T00:00:00+00:00",
            "zotero_plan": {"dry_run_commands": ["zcli import arxiv 2604.04979 --dry-run --format json"]}
        });

        let candidate = paper_candidate(1, InboxSource::Huggingface, "coding agent", &paper);

        assert_eq!(
            candidate["time"]["semantics"]["published_at"],
            "paper publication date from Hugging Face paper metadata"
        );
        assert_eq!(
            candidate["workflow"]["before_import"][0]["command"],
            "zcli import arxiv 2604.04979 --dry-run --format json"
        );
        assert_eq!(
            candidate["workflow"]["after_import_with_item_key"][2]["label"],
            "create_reading_context"
        );
    }

    #[test]
    fn seen_state_hides_recent_candidates_by_default() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.state_dir = Some(temp.path().to_path_buf());
        let handles = Vec::<String>::new();
        let topics = Vec::<String>::new();
        let paper = json!({
            "alphaxiv_id": "2605.00001",
            "canonical_id": "2605.00001v1",
            "title": "Agent Memory Harnesses",
            "metrics": {"public_total_votes": 3, "visits_7d": 10},
            "url": "https://www.alphaxiv.org/abs/2605.00001",
        });
        let candidate = paper_candidate(1, InboxSource::Alphaxiv, "", &paper);
        mark_candidates_seen(&config, &[candidate]).unwrap();

        let mut candidates = vec![paper_candidate(1, InboxSource::Alphaxiv, "", &paper)];
        let options = FetchOptions {
            config: &config,
            source: InboxSource::Alphaxiv,
            query: None,
            handles: &handles,
            configured_x_handles: &handles,
            tweet_limit: 0,
            limit: 30,
            fallback_sort: FeedSort::Hot,
            fallback_interval: "3 Days",
            topics: &topics,
            min_likes: None,
            min_github_stars: None,
            min_visits: None,
            since: None,
            days: None,
            date_field: DateField::Any,
            timeout: 1,
            context: false,
            code_overview: false,
            show_seen: false,
            show_existing: true,
            seen_days: 30,
            cache_overview: false,
            overview_ttl_days: 7,
            dry_run: true,
            execute: false,
        };

        let report = apply_inbox_filters(&mut candidates, &options).unwrap();

        assert!(candidates.is_empty());
        assert_eq!(report["hidden_seen_recently"], 1);
    }

    #[test]
    fn context_profile_reranks_and_explains_matches() {
        let mut candidates = vec![
            json!({"title": "Unrelated Paper", "triage": {"score": 100, "reasons": []}}),
            json!({"title": "Harness Memory for Coding Agents", "triage": {"score": 1, "reasons": []}}),
        ];
        let profile = ContextProfile {
            enabled: true,
            terms: vec!["harness".to_string(), "memory".to_string()],
            recent_titles: vec!["Agentic Harness Engineering".to_string()],
            queue_titles: vec![],
            unavailable_reason: None,
        };

        apply_context_profile(&mut candidates, &profile);

        assert_eq!(candidates[0]["title"], "Harness Memory for Coding Agents");
        assert_eq!(candidates[0]["context_match"]["score"], 50);
        assert_eq!(
            candidates[0]["triage"]["reasons"][0],
            "matches local Zotero/queue context"
        );
    }

    #[test]
    fn extracts_x_paper_links_into_candidates() {
        let tweet = json!({
            "id": "123",
            "createdAt": "2026-05-05T08:00:00.000Z",
            "text": "Great paper for coding agents\nhttps://arxiv.org/abs/2604.04979",
            "author": {"username": "paperbot"},
            "likeCount": 42,
            "retweetCount": 5
        });

        let papers = x_papers_from_tweet(&tweet, "coding agents");

        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0]["source"], "x");
        assert_eq!(papers[0]["arxiv_id"], "2604.04979");
        assert_eq!(papers[0]["tweet_url"], "https://x.com/paperbot/status/123");
    }

    #[test]
    fn x_handle_config_normalizes_and_dedupes() {
        let mut config = Config::default();
        add_x_handle(&mut config, "@paperbot").unwrap();
        add_x_handle(&mut config, "paperbot").unwrap();
        add_x_handle(&mut config, "agent_papers").unwrap();
        remove_x_handle(&mut config, "@paperbot").unwrap();

        assert_eq!(config.inbox.x_handles, vec!["agent_papers"]);
    }

    #[test]
    fn parses_github_repo_slug_for_quick_overview() {
        assert_eq!(
            github_repo_slug("https://github.com/KRLabsOrg/squeez"),
            Some("KRLabsOrg/squeez".to_string())
        );
        assert_eq!(
            github_repo_slug("git@github.com:owner/repo.git"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn concise_http_error_extracts_json_message() {
        let error = concise_http_error(
            r#"{"message":"API rate limit exceeded","documentation_url":"https://example.com"}"#,
        );

        assert_eq!(error, "GitHub API rate limit exceeded");
    }

    #[test]
    fn discussion_profile_builds_queries_from_title_and_arxiv() {
        let profile = paper_query_profile(
            "Squeez: Task-Conditioned Tool-Output Pruning for Coding Agents 2604.04979",
        );
        let queries = discussion_search_queries(
            &profile,
            &["paper_author".to_string()],
            parse_datetime("2026-05-01").as_ref(),
        );

        assert_eq!(profile.arxiv_id, Some("2604.04979".to_string()));
        assert!(queries.iter().any(|query| query.contains("2604.04979")));
        assert!(queries
            .iter()
            .any(|query| query.contains("from:paper_author")));
        assert!(queries
            .iter()
            .any(|query| query.contains("since:2026-05-01")));
    }

    #[test]
    fn discussion_items_prioritize_questions_and_author_answers() {
        let root = json!({
            "tweet_id": "1",
            "url": "https://x.com/author/status/1",
            "author": "@author"
        });
        let replies = vec![
            json!({
                "id": "2",
                "author": {"username": "reader"},
                "text": "How does this compare to the baseline and can we reproduce it?",
                "likeCount": 7
            }),
            json!({
                "id": "3",
                "author": {"username": "author"},
                "text": "We found the main limitation is memory cost, because the benchmark is long.",
                "likeCount": 1
            }),
            json!({
                "id": "4",
                "author": {"username": "reader"},
                "text": "congrats!"
            }),
        ];

        let items = discussion_items_from_tweets(replies, &root, "reply");

        assert_eq!(items.len(), 2);
        assert!(items[0]["value_labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "question"));
        assert!(!items[0]["value_labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "possible_author_answer"));
        assert!(items[1]["value_labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "possible_author_answer"));
    }
}
