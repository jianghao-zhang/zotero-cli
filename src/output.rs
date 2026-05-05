use anyhow::Result;
use clap::ValueEnum;
use serde_json::Value;
use std::io::IsTerminal;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Auto,
    Json,
    Pretty,
    Text,
}

pub fn print_value(value: &Value, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Auto if std::io::stdout().is_terminal() => print_text(value),
        OutputFormat::Auto => println!("{}", serde_json::to_string(value)?),
        OutputFormat::Json => println!("{}", serde_json::to_string(value)?),
        OutputFormat::Pretty => println!("{}", serde_json::to_string_pretty(value)?),
        OutputFormat::Text => print_text(value),
    }
    Ok(())
}

fn print_text(value: &Value) {
    if print_doctor(value) {
        return;
    }
    if print_setup(value) {
        return;
    }
    if print_helper(value) {
        return;
    }
    if print_write(value) {
        return;
    }
    if print_mirror_status(value) {
        return;
    }
    if print_examples(value) {
        return;
    }
    if print_alphaxiv(value) {
        return;
    }
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        println!("{message}");
        return;
    }
    if let Some(markdown) = value.get("markdown").and_then(Value::as_str) {
        print!("{markdown}");
        if !markdown.ends_with('\n') {
            println!();
        }
        return;
    }
    if let Some(items) = value.get("items").and_then(Value::as_array) {
        for item in items {
            let key = item.get("key").and_then(Value::as_str).unwrap_or("-");
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled");
            let year = item
                .get("year")
                .and_then(Value::as_i64)
                .map(|y| y.to_string())
                .unwrap_or_else(|| "-".to_string());
            println!("{key}\t{year}\t{title}");
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    );
}

fn print_doctor(value: &Value) -> bool {
    if value.get("mode").and_then(Value::as_str) != Some("local_read_only") {
        return false;
    }

    println!("zcli doctor");
    println!();

    if let Some(path) = value.get("config_path").and_then(Value::as_str) {
        println!("Config");
        println!("  path: {path}");
        println!();
    }

    if let Some(zotero) = value.get("zotero") {
        println!("Zotero");
        print_status_path(
            "database",
            zotero.get("db_available").and_then(Value::as_bool),
            zotero.get("db_path").and_then(Value::as_str),
        );
        print_status_path(
            "storage",
            zotero.get("storage_available").and_then(Value::as_bool),
            zotero.get("storage_path").and_then(Value::as_str),
        );
        println!();
    }

    if let Some(web_api) = value.get("web_api") {
        let enabled = web_api
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let key_present = web_api
            .get("api_key_present")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        println!("Zotero Web API");
        println!("  status: {}", if enabled { "enabled" } else { "disabled" });
        println!(
            "  library: {} {}",
            web_api
                .get("library_type")
                .and_then(Value::as_str)
                .unwrap_or("user"),
            web_api
                .get("library_id")
                .and_then(Value::as_str)
                .unwrap_or("(not set)")
        );
        println!(
            "  api key: {}{}",
            if key_present { "present" } else { "missing" },
            web_api
                .get("api_key_env")
                .and_then(Value::as_str)
                .map(|name| format!(" via {name}"))
                .unwrap_or_default()
        );
        if let Some(url) = web_api.get("api_key_url").and_then(Value::as_str) {
            println!("  get key: {url}");
        }
        if let Some(url) = web_api.get("library_id_help_url").and_then(Value::as_str) {
            println!("  find library id: {url}");
        }
        println!("  core commands use network: no");
        println!();
    }

    if let Some(helper) = value.get("helper") {
        println!("Zotero helper plugin");
        println!(
            "  status: {}",
            helper
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        print_status_path(
            "source",
            helper.get("source_exists").and_then(Value::as_bool),
            helper.get("source").and_then(Value::as_str),
        );
        if let Some(endpoint) = helper.get("endpoint").and_then(Value::as_str) {
            println!("  endpoint: {endpoint}");
        }
        print_status_path(
            "token",
            helper.get("token_present").and_then(Value::as_bool),
            helper.get("token_path").and_then(Value::as_str),
        );
        if let Some(performance) = helper.get("performance") {
            if let Some(mode) = performance.get("mode").and_then(Value::as_str) {
                println!("  mode: {mode}");
            }
        }
        println!("  required for core commands: no");
        println!();
    }

    if let Some(lfz) = value.get("lfz") {
        println!("llm-for-zotero");
        println!(
            "  status: {}",
            lfz.get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
        print_status_path(
            "runtime",
            lfz.get("runtime_exists").and_then(Value::as_bool),
            lfz.get("runtime_dir").and_then(Value::as_str),
        );
        if let Some(tables) = lfz.get("tables").and_then(Value::as_array) {
            let present = tables
                .iter()
                .filter(|table| table.get("exists").and_then(Value::as_bool) == Some(true))
                .count();
            let with_rows = tables
                .iter()
                .filter(|table| table.get("has_rows").and_then(Value::as_bool) == Some(true))
                .count();
            println!(
                "  tables: {present}/{} present, {with_rows} with rows",
                tables.len()
            );
        }
        println!();
    }

    println!("Boundaries");
    println!("  core Zotero access: local read-only");
    println!("  MCP server: no");
    println!("  required HTTP bridge: no");
    println!("  optional helper endpoint: yes, only if installed");
    println!("  imports/mutations: dry-run-first only");
    println!();
    println!("Useful next");
    println!(
        "  find a paper: zcli resolve \"title, short title, citation key, DOI, arXiv, URL, or path\""
    );
    println!("  paper view:   zcli paper ITEMKEY --format pretty");
    println!("  agent pack:   zcli context ITEMKEY --budget 40k --format json");
    true
}

fn print_write(value: &Value) -> bool {
    let Some(op) = value.get("helper_op").and_then(Value::as_str) else {
        return false;
    };
    let dry_run = value
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    println!("zcli write");
    println!();
    println!("  operation: {op}");
    println!("  dry run: {}", yes_no(dry_run));
    if dry_run {
        println!("  executed: no");
        println!("  helper required for execute: yes");
        if let Some(preview) = value.get("preview") {
            if let Some(target) = preview.get("target") {
                let key = target.get("key").and_then(Value::as_str).unwrap_or("-");
                let title = target
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled");
                println!("  target: {key} {title}");
            }
            if let Some(command) = preview.get("execute_command").and_then(Value::as_str) {
                println!("  execute: {command}");
            }
        }
    } else {
        println!("  executed: yes");
        if let Some(result) = value.get("result") {
            println!(
                "  helper result: {}",
                if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    "ok"
                } else {
                    "check JSON output"
                }
            );
        }
    }
    true
}

fn print_helper(value: &Value) -> bool {
    if value.get("optional").and_then(Value::as_bool) != Some(true)
        || value
            .get("capabilities")
            .and_then(Value::as_array)
            .is_none()
        || value.get("source").is_none()
    {
        return false;
    }

    println!("zcli helper");
    println!();
    println!(
        "  status: {}",
        value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    );
    print_status_path(
        "source",
        value.get("source_exists").and_then(Value::as_bool),
        value.get("source").and_then(Value::as_str),
    );
    if let Some(endpoint) = value.get("endpoint").and_then(Value::as_str) {
        println!("  endpoint: {endpoint}");
    }
    print_status_path(
        "token",
        value.get("token_present").and_then(Value::as_bool),
        value.get("token_path").and_then(Value::as_str),
    );
    if let Some(performance) = value.get("performance") {
        if let Some(mode) = performance.get("mode").and_then(Value::as_str) {
            println!("  mode: {mode}");
        }
        if performance
            .get("batch_supported")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            println!("  batch: supported");
        }
        if performance
            .get("compact_execute_responses")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            println!("  execute responses: compact");
        }
    }
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        println!("  error: {error}");
    }
    println!();
    println!("Capabilities");
    if let Some(caps) = value.get("capabilities").and_then(Value::as_array) {
        for cap in caps {
            if let Some(cap) = cap.as_str() {
                println!("  {cap}");
            }
        }
    }
    println!();
    println!("Useful next");
    println!("  preview install: zcli helper install --dry-run");
    println!("  package XPI:     zcli helper package --dry-run");
    true
}

fn print_mirror_status(value: &Value) -> bool {
    if value.get("configured").and_then(Value::as_bool).is_none()
        || value.get("index_path").is_none()
    {
        return false;
    }
    println!("zcli mirror");
    println!();
    print_status_path(
        "root",
        value.get("root_exists").and_then(Value::as_bool),
        value.get("mirror_root").and_then(Value::as_str),
    );
    print_status_path(
        "index",
        value.get("index_exists").and_then(Value::as_bool),
        value.get("index_path").and_then(Value::as_str),
    );
    if let Some(markdown) = value.get("markdown") {
        println!(
            "  markdown: {} via {}",
            markdown
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or("paper.md"),
            markdown
                .get("enable_with")
                .and_then(Value::as_str)
                .unwrap_or("--write-markdown")
        );
    }
    if let Some(auto) = value.get("auto_update") {
        println!(
            "  auto update: foreground watcher, every {}s by default",
            auto.get("default_interval_secs")
                .and_then(Value::as_i64)
                .unwrap_or(60)
        );
    }
    println!();
    println!("Useful next");
    println!("  preview: zcli mirror sync --dry-run");
    println!("  update:  zcli mirror sync --write-markdown");
    println!("  watch:   zcli mirror watch --write-markdown");
    true
}

fn print_examples(value: &Value) -> bool {
    let Some(examples) = value.get("examples").and_then(Value::as_array) else {
        return false;
    };
    println!("zcli examples");
    for example in examples {
        let name = example.get("name").and_then(Value::as_str).unwrap_or("-");
        let command = example.get("command").and_then(Value::as_str).unwrap_or("");
        println!("  {name}: {command}");
    }
    true
}

fn print_alphaxiv(value: &Value) -> bool {
    if value.get("source").and_then(Value::as_str) != Some("alphaxiv") {
        return false;
    }
    if let Some(papers) = value.get("papers").and_then(Value::as_array) {
        let count = value
            .get("count")
            .and_then(Value::as_i64)
            .unwrap_or(papers.len() as i64);
        if let Some(mode) = value.get("mode").and_then(Value::as_str) {
            println!("alphaXiv {mode}: {count} papers");
            if mode == "brief" {
                if let Some(window) = value.get("time_window") {
                    let since = window
                        .get("since")
                        .and_then(Value::as_str)
                        .and_then(|value| value.split('T').next())
                        .unwrap_or("-");
                    let date_field = window
                        .get("date_field")
                        .and_then(Value::as_str)
                        .unwrap_or("-");
                    println!("window: {date_field} since {since}");
                }
                if let Some(counts) = value.get("triage_counts") {
                    println!(
                        "triage: read_now:{} skim:{} watch:{}",
                        counts.get("read_now").and_then(Value::as_i64).unwrap_or(0),
                        counts.get("skim").and_then(Value::as_i64).unwrap_or(0),
                        counts.get("watch").and_then(Value::as_i64).unwrap_or(0)
                    );
                }
                println!();
            }
        } else if let Some(query) = value.get("query").and_then(Value::as_str) {
            println!("alphaXiv search: {query} ({count} papers)");
        } else {
            println!("alphaXiv papers: {count}");
        }
        for (idx, paper) in papers.iter().enumerate() {
            let id = paper
                .get("alphaxiv_id")
                .and_then(Value::as_str)
                .unwrap_or("-");
            let title = paper
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled");
            let metrics = paper.get("metrics").unwrap_or(&Value::Null);
            let likes = metrics
                .get("public_total_votes")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let stars = metrics
                .get("github_stars")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let visits = metrics
                .get("visits_7d")
                .or_else(|| metrics.get("visits_all"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let date = paper
                .get("first_seen_at")
                .or_else(|| paper.get("published_at"))
                .or_else(|| paper.get("updated_at"))
                .and_then(Value::as_str)
                .and_then(|value| value.split('T').next())
                .unwrap_or("-");
            let lane = paper
                .pointer("/triage/lane")
                .and_then(Value::as_str)
                .map(|lane| format!("  {lane}"))
                .unwrap_or_default();
            println!(
                "{}. {}  {}  likes:{} stars:{} visits:{}{}",
                idx + 1,
                id,
                date,
                likes,
                stars,
                visits,
                lane
            );
            println!("   {title}");
            if let Some(reasons) = paper.pointer("/triage/reasons").and_then(Value::as_array) {
                let reasons = reasons
                    .iter()
                    .filter_map(Value::as_str)
                    .take(3)
                    .collect::<Vec<_>>();
                if !reasons.is_empty() {
                    println!("   why: {}", reasons.join("; "));
                }
            }
            if let Some(url) = paper.get("url").and_then(Value::as_str) {
                println!("   {url}");
            }
            if let Some(command) = paper
                .pointer("/zotero_plan/dry_run_commands/0")
                .and_then(Value::as_str)
            {
                println!("   {command}");
            }
        }
        if let Some(hint) = value.get("selection_hint").and_then(Value::as_str) {
            println!();
            println!("{hint}");
        }
        return true;
    }
    if value.get("import_strategy").is_some() {
        let id = value.get("id").and_then(Value::as_str).unwrap_or("-");
        println!("alphaXiv Zotero plan: {id}");
        if let Some(strategy) = value.get("import_strategy").and_then(Value::as_str) {
            println!("  strategy: {strategy}");
        }
        if let Some(commands) = value.get("dry_run_commands").and_then(Value::as_array) {
            for command in commands.iter().filter_map(Value::as_str) {
                println!("  {command}");
            }
        }
        return true;
    }
    false
}

fn print_setup(value: &Value) -> bool {
    if !value.get("wrote_config").is_some() || !value.get("skill_installs").is_some() {
        return false;
    }

    println!("zcli setup");
    println!(
        "  config: {}",
        value
            .get("config_path")
            .and_then(Value::as_str)
            .unwrap_or("(unknown)")
    );
    println!(
        "  wrote config: {}",
        yes_no(
            value
                .get("wrote_config")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        )
    );
    println!(
        "  dry run: {}",
        yes_no(
            value
                .get("dry_run")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        )
    );

    if let Some(config) = value.get("config") {
        println!();
        println!("Configured");
        print_status_path(
            "Zotero database",
            config
                .get("zotero_db_path")
                .and_then(Value::as_str)
                .map(|_| true),
            config.get("zotero_db_path").and_then(Value::as_str),
        );
        print_status_path(
            "Zotero storage",
            config
                .get("zotero_storage_path")
                .and_then(Value::as_str)
                .map(|_| true),
            config.get("zotero_storage_path").and_then(Value::as_str),
        );
        print_status_path(
            "mirror root",
            config
                .get("mirror_root")
                .and_then(Value::as_str)
                .map(|_| true),
            config.get("mirror_root").and_then(Value::as_str),
        );
        if let Some(web_api) = config.get("web_api") {
            println!(
                "  Web API: {}",
                if web_api
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
        if let Some(lfz) = config.get("lfz") {
            println!(
                "  llm-for-zotero: {}",
                if lfz.get("enabled").and_then(Value::as_bool).unwrap_or(false) {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
    }

    if let Some(installs) = value.get("skill_installs").and_then(Value::as_array) {
        println!();
        println!("Skill installs: {}", installs.len());
    }

    true
}

fn print_status_path(label: &str, ok: Option<bool>, path: Option<&str>) {
    let status = match ok {
        Some(true) => "ok",
        Some(false) => "missing",
        None => "not set",
    };
    if let Some(path) = path {
        println!("  {label}: {status} {path}");
    } else {
        println!("  {label}: {status}");
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
