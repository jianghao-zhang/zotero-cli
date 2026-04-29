use assert_cmd::Command;
use rusqlite::{params, Connection};
use serde_json::Value;
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    _mirror_dir: TempDir,
    db: std::path::PathBuf,
    storage: std::path::PathBuf,
    config: std::path::PathBuf,
    mirror: std::path::PathBuf,
}

impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let dir = TempDir::new()?;
        let mirror_dir = TempDir::new()?;
        let db = dir.path().join("zotero.sqlite");
        let storage = dir.path().join("storage");
        let config = dir.path().join("config.toml");
        let mirror = mirror_dir.path().join("mirror");
        std::fs::create_dir_all(storage.join("ATTACH01"))?;
        std::fs::write(
            storage.join("ATTACH01").join("paper.pdf"),
            "full text about agent memory and Zotero automation",
        )?;
        std::fs::write(
            storage.join("ATTACH01").join(".zotero-ft-cache"),
            "cached PDF text about agent memory systems",
        )?;

        let conn = Connection::open(&db)?;
        create_schema(&conn)?;
        seed_data(&conn)?;

        Ok(Self {
            _dir: dir,
            _mirror_dir: mirror_dir,
            db,
            storage,
            config,
            mirror,
        })
    }

    fn cmd(&self) -> anyhow::Result<Command> {
        let mut cmd = Command::cargo_bin("zcli")?;
        cmd.arg("--config")
            .arg(&self.config)
            .arg("--db")
            .arg(&self.db)
            .arg("--storage")
            .arg(&self.storage)
            .env("ZCLI_CACHE_DIR", self._dir.path().join("cache"))
            .env("ZCLI_STATE_DIR", self._dir.path().join("state"));
        Ok(cmd)
    }
}

#[test]
fn doctor_and_web_api_config_are_json_first() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture
        .cmd()?
        .args([
            "config",
            "web-api",
            "--enable",
            "--library-type",
            "user",
            "--library-id",
            "1234567",
            "--api-key-env",
            "ZOTERO_API_KEY",
        ])
        .assert()
        .success();

    let output = fixture
        .cmd()?
        .env("ZOTERO_API_KEY", "secret")
        .arg("doctor")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["mode"], "local_read_only");
    assert_eq!(value["web_api"]["api_key_present"], true);
    assert_eq!(
        value["web_api"]["api_key_url"],
        "https://www.zotero.org/settings/keys"
    );
    assert_eq!(
        value["web_api"]["library_id_help_url"],
        "https://www.zotero.org/support/dev/web_api/v3/basics#user_and_group_library_urls"
    );
    assert_eq!(value["web_api"]["stored_api_key"], Value::Null);

    let output = fixture
        .cmd()?
        .args(["--format", "text", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output)?;
    assert!(text.contains("zcli doctor"));
    assert!(text.contains("core Zotero access: local read-only"));
    Ok(())
}

#[test]
fn helper_plugin_commands_are_dry_run_first() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let profile = fixture._dir.path().join("zotero-profile");
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("helper")
        .join("zcli-helper-zotero")
        .join("manifest.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
    assert_eq!(
        manifest["applications"]["zotero"]["id"],
        "zcli-helper@zotero-cli.local"
    );
    assert!(manifest["applications"]["zotero"]["update_url"]
        .as_str()
        .is_some_and(|url| url.starts_with("https://")));
    assert!(manifest["applications"]["zotero"]["strict_max_version"].is_string());

    let output = fixture
        .cmd()?
        .args(["helper", "doctor"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["optional"], true);
    assert_eq!(value["safety"]["arbitrary_js"], false);
    assert_eq!(value["safety"]["sqlite_writes"], false);
    assert_eq!(value["performance"]["mode"], "fast");
    assert_eq!(value["performance"]["batch_supported"], true);
    assert_eq!(value["source_exists"], true);

    let output = fixture
        .cmd()?
        .arg("helper")
        .arg("install")
        .arg("--dry-run")
        .arg("--profile")
        .arg(&profile)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["helper_id"], "zcli-helper@zotero-cli.local");
    Ok(())
}

#[test]
fn write_commands_preview_without_running_helper() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture
        .cmd()?
        .args(["write", "tags", "ITEM0001", "--add", "review", "--dry-run"])
        .assert()
        .success();

    let output = fixture
        .cmd()?
        .args(["write", "tags", "ITEM0001", "--add", "review", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["helper_required_for_execute"], true);
    assert_eq!(value["helper_op"], "apply_tags");
    assert_eq!(value["preview"]["target"]["key"], "ITEM0001");

    let attachment = fixture.storage.join("ATTACH01").join("paper.pdf");
    let output = fixture
        .cmd()?
        .arg("write")
        .arg("attach")
        .arg("ITEM0001")
        .arg(&attachment)
        .args(["--mode", "import", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["helper_op"], "import_local_files");
    assert_eq!(value["preview"]["file_exists"], true);

    let output = fixture
        .cmd()?
        .args([
            "write",
            "rename-attachment",
            "ATTACH01",
            "--name",
            "paper-renamed.pdf",
            "--dry-run",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["helper_op"], "rename_attachment");
    assert_eq!(value["preview"]["attachment_key"], "ATTACH01");

    fixture
        .cmd()?
        .args(["write", "trash", "ITEM0001"])
        .assert()
        .failure();
    Ok(())
}

#[test]
fn import_commands_preview_without_running_helper() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;

    fixture
        .cmd()?
        .args(["import", "arxiv", "2604.06240"])
        .assert()
        .failure();

    let output = fixture
        .cmd()?
        .args(["import", "arxiv", "2601.12345", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["helper_op"], "import_identifiers");
    assert_eq!(value["preview"]["sources"][0]["status"], "skip_existing");
    assert_eq!(value["params"]["identifiers"].as_array().unwrap().len(), 0);

    let output = fixture
        .cmd()?
        .args([
            "import",
            "arxiv",
            "https://arxiv.org/abs/2604.06240",
            "--collection",
            "Agent Papers",
            "--tag",
            "unread",
            "--dry-run",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["preview"]["sources"][0]["kind"], "arxiv");
    assert_eq!(value["preview"]["sources"][0]["value"], "2604.06240");
    assert_eq!(value["params"]["identifiers"][0]["kind"], "arxiv");
    assert_eq!(value["params"]["collections"][0], "Agent Papers");
    assert_eq!(value["params"]["tags"][0], "unread");

    let new_pdf = fixture._dir.path().join("new-paper.pdf");
    std::fs::write(&new_pdf, "new pdf")?;
    let output = fixture
        .cmd()?
        .arg("import")
        .arg("pdf")
        .arg(&new_pdf)
        .args(["--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["helper_op"], "import_pdfs");
    assert_eq!(value["preview"]["recognize_metadata"], true);
    assert_eq!(value["preview"]["sources"][0]["kind"], "local_pdf");
    assert_eq!(value["preview"]["sources"][0]["exists"], true);
    assert_eq!(
        value["params"]["sources"][0]["path"],
        new_pdf.canonicalize()?.display().to_string()
    );

    let output = fixture
        .cmd()?
        .args([
            "import",
            "url",
            "https://doi.org/10.1234/example",
            "--dry-run",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["helper_op"], "import_urls");
    assert_eq!(value["preview"]["sources"][0]["kind"], "doi");
    assert_eq!(value["preview"]["sources"][0]["status"], "skip_existing");
    assert_eq!(value["params"]["urls"].as_array().unwrap().len(), 0);

    Ok(())
}

#[test]
fn search_and_item_commands_read_local_fixture() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args(["search", "list", "agent", "--limit", "5"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["items"][0]["key"], "ITEM0001");
    assert_eq!(value["items"][0]["title"], "Agent Memory for Research");
    assert_eq!(value["items"][0]["short_title"], "Agent Memory");
    assert_eq!(value["items"][0]["citation_key"], "lovelace2026AgentMemory");

    let output = fixture
        .cmd()?
        .args(["resolve", "lovelace2026AgentMemory"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["matches"][0]["item"]["key"], "ITEM0001");
    assert_eq!(value["matches"][0]["reasons"][0], "citation_key_exact");

    let output = fixture
        .cmd()?
        .args(["resolve", "Agent Memory"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert!(value["matches"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "short_title_exact"));

    let output = fixture
        .cmd()?
        .args(["find", "paper", "lovelace2026AgentMemory"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["mode"], "local_hybrid_lexical");
    assert_eq!(value["hits"][0]["item"]["key"], "ITEM0001");
    assert!(value["hits"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "citation_key_exact"));

    let output = fixture
        .cmd()?
        .args(["find", "paper", "research systems"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["hits"][0]["item"]["key"], "ITEM0001");
    assert!(value["hits"][0]["matched"].get("abstract").is_some());

    let output = fixture
        .cmd()?
        .args(["find", "paper", "AMFR"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["hits"][0]["item"]["key"], "ITEM0001");
    assert!(value["hits"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "title_acronym_exact"));

    let output = fixture
        .cmd()?
        .args(["item", "extract", "ITEM0001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert!(value["extract"]["text"]
        .as_str()
        .unwrap()
        .contains("cached PDF text"));

    let output = fixture
        .cmd()?
        .args(["item", "markdown", "ITEM0001", "--max-chars", "2000"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["markdown_meta"]["source"], "zcli_fallback");
    assert!(value["markdown"]
        .as_str()
        .unwrap()
        .contains("# Agent Memory for Research"));

    let mineru_dir = fixture._dir.path().join("llm-for-zotero-mineru").join("2");
    std::fs::create_dir_all(&mineru_dir)?;
    std::fs::write(mineru_dir.join("full.md"), "# MinerU full markdown\n\nbody")?;
    std::fs::write(
        &fixture.config,
        format!(
            "[lfz]\nenabled = true\nzotero_data_dir = \"{}\"\n",
            fixture._dir.path().display()
        ),
    )?;
    let output = fixture
        .cmd()?
        .args(["item", "markdown", "ITEM0001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["markdown_meta"]["source"], "llm_for_zotero_full_md");
    assert_eq!(
        value["markdown"].as_str().unwrap(),
        "# MinerU full markdown\n\nbody"
    );
    Ok(())
}

#[test]
fn local_index_update_search_and_get_fixture() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args(["index", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["exists"], false);
    assert_eq!(value["backend"], "sqlite_fts5_bm25");

    let output = fixture
        .cmd()?
        .args(["index", "update"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["indexed"], 1);
    assert_eq!(value["chunks_indexed"], 3);
    assert_eq!(value["chunks_with_page"], 1);
    assert_eq!(value["models_required"], false);

    let output = fixture
        .cmd()?
        .args(["index", "search", "lovelace2026AgentMemory"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["hits"][0]["item"]["key"], "ITEM0001");
    assert_eq!(
        value["hits"][0]["item"]["citation_key"],
        "lovelace2026AgentMemory"
    );

    let output = fixture
        .cmd()?
        .args(["index", "search", "research systems"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["hits"][0]["item"]["key"], "ITEM0001");
    assert!(value["hits"][0]["snippet"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("research"));

    let output = fixture
        .cmd()?
        .args(["index", "get", "lovelace2026AgentMemory"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["item"]["key"], "ITEM0001");
    assert_eq!(value["item"]["short_title"], "Agent Memory");
    assert_eq!(value["truncated"]["abstract"], false);

    let output = fixture
        .cmd()?
        .args(["index", "chunks", "annotation memory", "--item", "ITEM0001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["backend"], "sqlite_fts5_bm25_chunks");
    assert_eq!(value["hits"][0]["item"]["key"], "ITEM0001");
    assert_eq!(value["hits"][0]["source"], "annotation");
    assert_eq!(value["hits"][0]["page"], "1");
    assert_eq!(value["hits"][0]["has_page"], true);
    let chunk_id = value["hits"][0]["chunk_id"].as_str().unwrap().to_string();

    let output = fixture
        .cmd()?
        .args(["index", "chunk", &chunk_id])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["chunk_id"], chunk_id);
    assert_eq!(value["source"], "annotation");
    assert_eq!(value["page"], "1");
    assert!(value["text"]
        .as_str()
        .unwrap()
        .contains("annotation about memory"));
    Ok(())
}

#[test]
fn recap_reading_and_lfz_keep_separate_boundaries() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args([
            "recap",
            "reading",
            "--from",
            "2000-01-01",
            "--to",
            "2100-01-01",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    let entries = value["entries"].as_array().unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry["provenance"] == "annotation"));
    assert!(entries
        .iter()
        .any(|entry| entry["provenance"] == "metadata_modified"));
    assert!(value.get("lfz").is_none());
    assert_eq!(value["lfz_policy"]["included"], false);

    let output = fixture
        .cmd()?
        .args(["recap", "lfz", "--from", "2000-01-01", "--to", "2100-01-01"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["kind"], "lfz");
    assert_eq!(value["lfz"]["status"], "available");
    assert_eq!(value["lfz"]["compact"], true);
    assert_eq!(value["lfz"]["text_policy"], "compact_index");
    assert_eq!(
        value["lfz"]["expand_policy"],
        "use_turn_command_or_expand_command_for_one_specific_full_question_and_final_answer"
    );
    assert_eq!(
        value["lfz"]["paper_groups"][0]["group_key"],
        "item:ITEM0001"
    );
    assert_eq!(
        value["lfz"]["paper_groups"][0]["expand_commands"][0],
        "zcli lfz turn claude:1"
    );
    assert_eq!(
        value["lfz"]["questions"][0]["conversation_system"],
        "claude_code"
    );
    assert_eq!(
        value["lfz"]["questions"][0]["expand_command"],
        "zcli lfz turn claude:1"
    );
    assert_eq!(value["lfz"]["questions"][0]["full_text_available"], true);
    assert!(
        value["lfz"]["questions"][0]["text_estimated_tokens"]
            .as_u64()
            .unwrap()
            > 0
    );

    let output = fixture
        .cmd()?
        .args([
            "recap",
            "lfz",
            "--item",
            "ITEM0001",
            "--full-text",
            "--from",
            "2000-01-01",
            "--to",
            "2100-01-01",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["item_filter"]["key"], "ITEM0001");
    assert_eq!(value["lfz"]["counts"]["messages"], 1);
    assert_eq!(value["lfz"]["questions"][0]["message_ref"], "claude:1");
    assert_eq!(
        value["lfz"]["questions"][0]["turn_command"],
        "zcli lfz turn claude:1"
    );
    assert_eq!(value["lfz"]["questions"][0]["text"], "recap this paper");
    assert_eq!(value["lfz"]["agent_finals"][0]["final_text"], "done");
    assert!(value["lfz"]["agent_finals"][0].get("events").is_none());
    assert!(value["lfz"]["agent_finals"][0]
        .get("events_included")
        .is_none());

    let output = fixture
        .cmd()?
        .args(["lfz", "turn", "claude:1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["lfz"]["question"]["text"], "recap this paper");
    assert_eq!(value["lfz"]["agent_finals"][0]["final_text"], "done");
    assert!(value["lfz"].get("include_events").is_none());
    assert!(value["lfz"]["agent_finals"][0].get("events").is_none());
    Ok(())
}

#[test]
fn skill_install_dry_run_reports_target_path() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args(["skill", "install", "--target", "codex", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["target"], "codex");
    assert!(value["target_path"].as_str().unwrap().contains(".codex"));
    Ok(())
}

#[test]
fn lfz_skill_install_uses_profile_runtime_targets() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let runtime = fixture._dir.path().join("agent-runtime");
    std::fs::create_dir_all(runtime.join("profile-alpha").join(".claude"))?;
    std::fs::create_dir_all(
        fixture
            ._dir
            .path()
            .join("Library/Application Support/Zotero/Profiles/test.default/agent-runtime/.claude"),
    )?;
    std::fs::write(
        &fixture.config,
        format!(
            r#"[lfz]
enabled = true
zotero_data_dir = "{}"
claude_runtime_dir = "{}"
"#,
            fixture._dir.path().display(),
            runtime.display()
        ),
    )?;

    let output = fixture
        .cmd()?
        .env("HOME", fixture._dir.path())
        .args(["skill", "install", "--target", "lfz", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["target"], "lfz");
    assert!(value["source"].as_str().unwrap().contains("zotero-cli-lfz"));
    assert!(value["target_path"]
        .as_str()
        .unwrap()
        .contains("profile-alpha/.claude/skills/zotero-cli"));
    let rendered_targets = serde_json::to_string(&value["target_paths"])?;
    assert!(!rendered_targets.contains("Application Support/Zotero/Profiles"));
    assert_eq!(value["target_paths"].as_array().unwrap().len(), 1);
    Ok(())
}

#[test]
fn public_command_smoke_outputs_json() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    for args in [
        vec!["examples"],
        vec!["resolve", "Agent Memory"],
        vec!["find", "paper", "memory"],
        vec!["paper", "ITEM0001"],
        vec!["context", "ITEM0001", "--budget", "5k"],
        vec!["index", "status"],
        vec!["markdown", "status", "ITEM0001"],
        vec!["open", "ITEM0001", "--dry-run"],
        vec!["reveal", "ITEM0001", "--dry-run"],
        vec!["search", "grep", "memory"],
        vec!["search", "context", "ITEM0001", "memory"],
        vec!["item", "annotations", "ITEM0001"],
        vec!["item", "notes", "ITEM0001"],
        vec!["item", "attachments", "ITEM0001"],
        vec!["item", "bibtex", "ITEM0001"],
        vec!["item", "markdown", "ITEM0001"],
        vec!["collection", "list"],
        vec!["collection", "items", "COLL0001"],
        vec!["tags", "list"],
        vec!["tags", "items", "agents"],
        vec!["recent", "--days", "9999"],
        vec!["recap", "today"],
        vec!["recap", "week"],
        vec!["lfz", "doctor"],
        vec!["lfz", "turn", "claude:1", "--budget", "5k"],
        vec!["lfz", "turns", "--item", "ITEM0001"],
        vec!["queue", "list"],
        vec!["todo", "list"],
        vec!["export", "pack", "ITEM0001", "--for", "codex", "--dry-run"],
        vec!["skill", "doctor"],
        vec!["inbox", "status"],
        vec!["inbox", "fetch", "--dry-run"],
    ] {
        let output = fixture
            .cmd()?
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: Value = serde_json::from_slice(&output)?;
        assert_eq!(value["ok"], true);
    }

    let output = fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.mirror)
        .args(["mirror", "rebuild", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);

    let output = fixture
        .cmd()?
        .args(["mirror", "daemon-install", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    Ok(())
}

#[test]
fn setup_defaults_dry_run_is_non_interactive() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args(["setup", "--defaults", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);
    assert_eq!(value["wrote_config"], false);
    assert_eq!(
        value["config"]["zotero_db_path"],
        fixture.db.display().to_string()
    );
    assert_eq!(value["config"]["web_api"]["api_key_env"], "ZOTERO_API_KEY");
    assert_eq!(
        value["config"]["web_api"]["library_id_help_url"],
        "https://www.zotero.org/support/dev/web_api/v3/basics#user_and_group_library_urls"
    );
    Ok(())
}

#[test]
fn setup_existing_config_prompts_before_long_wizard() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.cmd()?.args(["config", "init"]).assert().success();

    let output = fixture
        .cmd()?
        .args(["setup"])
        .write_stdin("n\n")
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("Existing config found:"));
    assert!(stderr.contains("Overwrite existing config when setup finishes?"));
    assert!(!stderr.contains("Local Zotero library"));
    assert!(stderr.contains("answer yes to overwrite"));

    let output = fixture
        .cmd()?
        .args(["setup", "--no-skills"])
        .write_stdin("y\n\n\nn\nn\nn\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["wrote_config"], true);
    assert!(value["notes"][0]
        .as_str()
        .unwrap()
        .contains("approved overwriting"));
    Ok(())
}

#[test]
fn config_commands_init_status_and_store_web_api_key() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args(["config", "init"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert!(fixture.config.exists());

    let output = fixture
        .cmd()?
        .args([
            "config",
            "web-api",
            "--enable",
            "--library-type",
            "group",
            "--library-id",
            "7654321",
            "--api-key-stdin",
        ])
        .write_stdin("stored-secret\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["web_api"]["enabled"], true);
    assert_eq!(value["web_api"]["library_type"], "group");
    assert_eq!(value["web_api"]["stored_api_key"], "<redacted>");

    let output = fixture
        .cmd()?
        .args(["config", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["config"]["web_api"]["stored_api_key"], "<redacted>");
    assert_eq!(
        value["config"]["web_api"]["library_id_help_url"],
        "https://www.zotero.org/support/dev/web_api/v3/basics#user_and_group_library_urls"
    );
    Ok(())
}

#[test]
fn mirror_rebuild_and_sync_write_expected_files() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.mirror)
        .args(["mirror", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["configured"], true);

    let output = fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.mirror)
        .args(["mirror", "rebuild", "--mode", "copy", "--write-markdown"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["dry_run"], false);
    assert!(fixture.mirror.join(".zcli-mirror-index.json").exists());
    assert!(fixture.mirror.join("Allin").exists());
    let first_dir = value["index"]["entries"][0]["dir"]
        .as_str()
        .map(std::path::PathBuf::from)
        .unwrap();
    assert!(first_dir.exists());
    assert!(first_dir.join("paper.md").exists());

    let output = fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.mirror)
        .args(["mirror", "sync", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["dry_run"], true);

    let conn = Connection::open(&fixture.db)?;
    conn.execute("DELETE FROM items WHERE itemID = 1", [])?;
    let output = fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.mirror)
        .args(["mirror", "sync", "--mode", "copy"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["stale_count"], 2);
    assert!(!first_dir.exists());

    let output = fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.mirror)
        .args(["mirror", "watch", "--once", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["ok"], true);
    assert_eq!(value["watch"]["foreground"], true);
    assert_eq!(value["events"].as_array().unwrap().len(), 1);

    fixture
        .cmd()?
        .arg("--mirror-root")
        .arg(&fixture.storage)
        .args(["mirror", "rebuild", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("unsafe mirror_root"));
    Ok(())
}

#[test]
fn recap_reading_include_lfz_and_inbox_execute_paths_are_explicit() -> anyhow::Result<()> {
    let configured = Fixture::new()?;
    std::fs::write(
        &configured.config,
        r#"[lfz]
enabled = true
claude_runtime_dir = "/tmp/lfz"
"#,
    )?;
    let output = configured
        .cmd()?
        .args([
            "recap",
            "reading",
            "--from",
            "2000-01-01",
            "--to",
            "2100-01-01",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["lfz_policy"]["enabled_in_config"], true);
    assert_eq!(value["lfz_policy"]["requested_by_flag"], false);
    assert_eq!(value["lfz_policy"]["included"], true);
    assert_eq!(value["lfz"]["status"], "available");

    let output = configured
        .cmd()?
        .args([
            "recap",
            "reading",
            "--no-lfz",
            "--from",
            "2000-01-01",
            "--to",
            "2100-01-01",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["lfz_policy"]["included"], false);
    assert!(value.get("lfz").is_none());

    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args([
            "recap",
            "reading",
            "--include-lfz",
            "--from",
            "2000-01-01",
            "--to",
            "2100-01-01",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["kind"], "reading");
    assert_eq!(value["lfz_policy"]["requested_by_flag"], true);
    assert_eq!(value["lfz_policy"]["included"], false);
    assert_eq!(
        value["lfz_policy"]["unavailable_reason"],
        "lfz_not_enabled_in_config"
    );
    assert!(value.get("lfz").is_none());

    let output = fixture
        .cmd()?
        .args(["inbox", "fetch", "--execute"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["executed"], false);
    Ok(())
}

#[test]
fn setup_can_write_temp_config_and_all_skill_targets_have_dry_run() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let output = fixture
        .cmd()?
        .args(["setup", "--defaults", "--force"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output)?;
    assert_eq!(value["wrote_config"], true);
    assert!(fixture.config.exists());

    for target in ["codex", "claude", "hermes", "lfz", "openclaw"] {
        let output = fixture
            .cmd()?
            .args(["skill", "install", "--target", target, "--dry-run"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: Value = serde_json::from_slice(&output)?;
        assert_eq!(value["ok"], true);
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["target"], target);
    }
    Ok(())
}

fn create_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE itemTypes(itemTypeID INTEGER PRIMARY KEY, typeName TEXT NOT NULL);
        CREATE TABLE items(itemID INTEGER PRIMARY KEY, itemTypeID INTEGER, dateAdded TEXT, dateModified TEXT, key TEXT NOT NULL);
        CREATE TABLE fields(fieldID INTEGER PRIMARY KEY, fieldName TEXT NOT NULL);
        CREATE TABLE itemData(itemID INTEGER, fieldID INTEGER, valueID INTEGER);
        CREATE TABLE itemDataValues(valueID INTEGER PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE creators(creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT, fieldMode INTEGER);
        CREATE TABLE itemCreators(itemID INTEGER, creatorID INTEGER, creatorTypeID INTEGER, orderIndex INTEGER);
        CREATE TABLE tags(tagID INTEGER PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE itemTags(itemID INTEGER, tagID INTEGER);
        CREATE TABLE collections(collectionID INTEGER PRIMARY KEY, key TEXT NOT NULL, collectionName TEXT NOT NULL, parentCollectionID INTEGER);
        CREATE TABLE collectionItems(collectionID INTEGER, itemID INTEGER);
        CREATE TABLE itemAttachments(itemID INTEGER, parentItemID INTEGER, linkMode INTEGER, contentType TEXT, path TEXT);
        CREATE TABLE itemNotes(itemID INTEGER, parentItemID INTEGER, title TEXT, note TEXT);
        CREATE TABLE itemAnnotations(itemID INTEGER, parentItemID INTEGER, annotationType TEXT, annotationText TEXT, annotationComment TEXT, annotationColor TEXT, annotationPageLabel TEXT, annotationPosition TEXT, dateModified TEXT);

        CREATE TABLE llm_for_zotero_claude_messages(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          conversation_key INTEGER NOT NULL,
          role TEXT NOT NULL,
          text TEXT NOT NULL,
          timestamp INTEGER NOT NULL,
          run_mode TEXT,
          agent_run_id TEXT,
          selected_text TEXT,
          selected_texts_json TEXT,
          selected_text_paper_contexts_json TEXT,
          paper_contexts_json TEXT,
          full_text_paper_contexts_json TEXT,
          model_name TEXT,
          model_entry_id TEXT,
          model_provider_label TEXT,
          reasoning_summary TEXT,
          context_tokens INTEGER,
          context_window INTEGER
        );
        CREATE TABLE llm_for_zotero_claude_conversations(
          conversation_key INTEGER PRIMARY KEY,
          library_id INTEGER NOT NULL,
          kind TEXT NOT NULL,
          paper_item_id INTEGER,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          title TEXT,
          provider_session_id TEXT,
          scoped_conversation_key TEXT,
          scope_type TEXT,
          scope_id TEXT,
          scope_label TEXT,
          cwd TEXT,
          model_name TEXT,
          effort TEXT
        );
        CREATE TABLE llm_for_zotero_agent_runs(
          run_id TEXT PRIMARY KEY,
          conversation_key INTEGER NOT NULL,
          mode TEXT NOT NULL,
          model_name TEXT,
          status TEXT NOT NULL,
          created_at INTEGER NOT NULL,
          completed_at INTEGER,
          final_text TEXT
        );
        CREATE TABLE llm_for_zotero_agent_run_events(
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id TEXT NOT NULL,
          seq INTEGER NOT NULL,
          event_type TEXT NOT NULL,
          payload_json TEXT NOT NULL,
          created_at INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn seed_data(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        r#"
        INSERT INTO itemTypes VALUES (1, 'journalArticle'), (2, 'attachment'), (3, 'annotation'), (4, 'note');
        INSERT INTO fields VALUES (1, 'title'), (2, 'abstractNote'), (3, 'date'), (4, 'DOI'), (5, 'url'), (6, 'extra'), (7, 'shortTitle'), (8, 'citationKey');
        INSERT INTO items VALUES (1, 1, '2026-04-01 10:00:00', '2026-04-20 10:00:00', 'ITEM0001');
        INSERT INTO items VALUES (2, 2, '2026-04-01 10:10:00', '2026-04-20 10:10:00', 'ATTACH01');
        INSERT INTO items VALUES (3, 3, '2026-04-20 11:00:00', '2026-04-20 11:00:00', 'ANNOT001');
        INSERT INTO items VALUES (4, 4, '2026-04-21 11:00:00', '2026-04-21 11:00:00', 'NOTE0001');
        INSERT INTO itemDataValues VALUES
          (1, 'Agent Memory for Research'),
          (2, 'A paper about agent memory systems.'),
          (3, '2026'),
          (4, '10.1234/example'),
          (5, 'https://arxiv.org/abs/2601.12345'),
          (6, 'arXiv:2601.12345'),
          (7, 'Agent Memory'),
          (8, 'lovelace2026AgentMemory');
        INSERT INTO itemData VALUES (1, 1, 1), (1, 2, 2), (1, 3, 3), (1, 4, 4), (1, 5, 5), (1, 6, 6), (1, 7, 7), (1, 8, 8);
        INSERT INTO creators VALUES (1, 'Ada', 'Lovelace', 0);
        INSERT INTO itemCreators VALUES (1, 1, 1, 0);
        INSERT INTO tags VALUES (1, 'agents'), (2, 'memory');
        INSERT INTO itemTags VALUES (1, 1), (1, 2);
        INSERT INTO collections VALUES (1, 'COLL0001', 'Agent Papers', NULL);
        INSERT INTO collectionItems VALUES (1, 1);
        INSERT INTO itemAttachments VALUES (2, 1, 1, 'application/pdf', 'storage:paper.pdf');
        INSERT INTO itemNotes VALUES (4, 1, 'Reading note', '<p>Important note about CLI agents.</p>');
        INSERT INTO itemAnnotations VALUES (3, 2, 'highlight', 'annotation about memory', 'follow up', '#ff0', '1', '{}', '2026-04-20 12:00:00');
        "#,
    )?;
    let ts = 1_776_700_800_000_i64;
    conn.execute(
        "INSERT INTO llm_for_zotero_claude_conversations
          (conversation_key, library_id, kind, paper_item_id, created_at, updated_at, title, provider_session_id, scoped_conversation_key, scope_type, scope_id, scope_label, cwd, model_name, effort)
         VALUES (?, 1, 'paper', 1, ?, ?, 'Claude paper chat', 'session-1', 'scope-1', 'paper', '1', 'Agent Memory for Research', '/tmp/lfz', 'claude-sonnet', 'medium')",
        params![3_000_000_001_i64, ts, ts],
    )?;
    conn.execute(
        "INSERT INTO llm_for_zotero_claude_messages
          (conversation_key, role, text, timestamp, run_mode, agent_run_id, selected_text, paper_contexts_json, model_name, model_provider_label)
         VALUES (?, 'user', 'recap this paper', ?, 'agent', 'run-1', 'selected text', '[{\"itemKey\":\"ITEM0001\"}]', 'claude-sonnet', 'Claude Code')",
        params![3_000_000_001_i64, ts],
    )?;
    conn.execute(
        "INSERT INTO llm_for_zotero_agent_runs VALUES ('run-1', ?, 'agent', 'claude-sonnet', 'completed', ?, ?, 'done')",
        params![3_000_000_001_i64, ts, ts + 1000],
    )?;
    conn.execute(
        "INSERT INTO llm_for_zotero_agent_run_events(run_id, seq, event_type, payload_json, created_at)
         VALUES ('run-1', 1, 'tool', '{\"name\":\"zotero_read\"}', ?)",
        params![ts + 1],
    )?;
    Ok(())
}
