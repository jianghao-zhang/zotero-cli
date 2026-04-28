use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension, Row};
use serde_json::{json, Value};

use crate::{config::Config, date_range::DateRange, zotero::ZoteroDb};

const UPSTREAM_MESSAGES: &str = "llm_for_zotero_chat_messages";
const LEGACY_MESSAGES: &str = "zoterollm_chat_messages";
const CLAUDE_MESSAGES: &str = "llm_for_zotero_claude_messages";
const CLAUDE_CONVERSATIONS: &str = "llm_for_zotero_claude_conversations";
const PAPER_CONVERSATIONS: &str = "llm_for_zotero_paper_conversations";
const GLOBAL_CONVERSATIONS: &str = "llm_for_zotero_global_conversations";
const AGENT_RUNS: &str = "llm_for_zotero_agent_runs";
const AGENT_EVENTS: &str = "llm_for_zotero_agent_run_events";

#[derive(Clone, Copy, Debug)]
pub struct RecapOptions {
    pub item_id: Option<i64>,
    pub compact: bool,
    pub limit: usize,
    pub full_text: bool,
    pub include_contexts: bool,
}

pub fn doctor(config: &Config, db: Option<&ZoteroDb>) -> Result<Value> {
    let runtime_dir = config.lfz.claude_runtime_dir.clone();
    let mut tables = Vec::new();
    if let Some(db) = db {
        for table in [
            UPSTREAM_MESSAGES,
            LEGACY_MESSAGES,
            CLAUDE_MESSAGES,
            CLAUDE_CONVERSATIONS,
            PAPER_CONVERSATIONS,
            GLOBAL_CONVERSATIONS,
            AGENT_RUNS,
            AGENT_EVENTS,
        ] {
            tables.push(json!({
                "name": table,
                "exists": db.table_exists(table),
                "has_rows": if db.table_exists(table) { table_has_rows(db, table).ok() } else { None },
            }));
        }
    }

    let has_messages = db
        .map(|db| {
            db.table_exists(UPSTREAM_MESSAGES)
                || db.table_exists(LEGACY_MESSAGES)
                || db.table_exists(CLAUDE_MESSAGES)
        })
        .unwrap_or(false);
    let has_claude = db
        .map(|db| db.table_exists(CLAUDE_MESSAGES) || db.table_exists(CLAUDE_CONVERSATIONS))
        .unwrap_or(false);
    let runtime_exists = runtime_dir.as_deref().map(Path::exists).unwrap_or(false);
    let status = match (has_messages, has_claude, runtime_exists) {
        (false, false, false) => "unavailable",
        (true, true, _) => "available",
        _ => "partial",
    };

    Ok(json!({
        "status": status,
        "optional": true,
        "runtime_dir": runtime_dir,
        "runtime_exists": runtime_exists,
        "tables": tables,
    }))
}

pub fn recap(
    config: &Config,
    db: &ZoteroDb,
    range: &DateRange,
    options: RecapOptions,
) -> Result<Value> {
    let status = doctor(config, Some(db))?;
    let status_label = status
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    if status_label == "unavailable" {
        return Ok(json!({
            "status": "unavailable",
            "reason": "llm-for-zotero chat tables and Claude runtime metadata were not found",
            "conversations": [],
            "messages": [],
            "agent_runs": [],
        }));
    }

    let conversations = filter_conversations(conversation_metadata(db)?, options.item_id);
    let mut messages = Vec::new();
    for table in [UPSTREAM_MESSAGES, LEGACY_MESSAGES, CLAUDE_MESSAGES] {
        if db.table_exists(table) {
            messages.extend(read_messages(
                db,
                table,
                range,
                &conversations,
                options.include_contexts,
                options.full_text,
            )?);
        }
    }
    messages.sort_by(|a, b| {
        a.get("timestamp")
            .and_then(Value::as_i64)
            .cmp(&b.get("timestamp").and_then(Value::as_i64))
    });

    let agent_runs = read_agent_runs(db, range, options.full_text, &conversations)?;
    let detail = json!({
        "status": status_label,
        "runtime_dir": config.lfz.claude_runtime_dir,
        "item_id": options.item_id,
        "compact": options.compact,
        "full_text": options.full_text,
        "text_policy": text_policy(options.full_text),
        "expand_policy": expand_policy(options.full_text),
        "trace_payloads": "disabled",
        "include_contexts": options.include_contexts,
        "conversations": conversations.values().collect::<Vec<_>>(),
        "messages": messages,
        "agent_runs": agent_runs,
        "provenance": "llm_for_zotero_sqlite",
    });
    if options.compact {
        Ok(compact_recap(&detail, options.limit))
    } else {
        Ok(detail)
    }
}

pub fn turn(
    config: &Config,
    db: &ZoteroDb,
    message_ref: &str,
    include_contexts: bool,
) -> Result<Value> {
    let status = doctor(config, Some(db))?;
    let (table, message_id) = parse_message_ref(message_ref)?;
    if !db.table_exists(table) {
        anyhow::bail!("llm-for-zotero message table not found: {table}");
    }
    let conversations = conversation_metadata(db)?;
    let source = read_message_by_id(
        db,
        table,
        message_id,
        &conversations,
        include_contexts,
        true,
    )?
    .ok_or_else(|| anyhow!("llm-for-zotero message not found: {message_ref}"))?;
    let conversation_key = source
        .get("conversation_key")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("message is missing conversation_key: {message_ref}"))?;
    let timestamp = source
        .get("timestamp")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("message is missing timestamp: {message_ref}"))?;
    let role = source.get("role").and_then(Value::as_str).unwrap_or("");

    let question = if role == "user" {
        source.clone()
    } else {
        previous_user_message(db, table, conversation_key, timestamp, &conversations)?
            .unwrap_or_else(|| source.clone())
    };
    let question_ts = question
        .get("timestamp")
        .and_then(Value::as_i64)
        .unwrap_or(timestamp);
    let next_user_ts = next_user_timestamp(db, table, conversation_key, question_ts)?;
    let answers = if role == "assistant" {
        vec![source.clone()]
    } else {
        assistant_messages_after(
            db,
            table,
            conversation_key,
            question_ts,
            next_user_ts,
            &conversations,
        )?
    };
    let runs = turn_agent_runs(
        db,
        conversation_key,
        question_ts,
        next_user_ts,
        &question,
        &answers,
        &conversations,
    )?;

    Ok(json!({
        "status": status.get("status").cloned().unwrap_or_else(|| json!("unavailable")),
        "runtime_dir": config.lfz.claude_runtime_dir,
        "message_ref": message_ref,
        "text_policy": "full_turn",
        "trace_payloads": "disabled",
        "include_contexts": include_contexts,
        "conversation": conversations.get(&conversation_key),
        "question": question,
        "answers": answers,
        "agent_finals": runs,
        "provenance": "llm_for_zotero_sqlite",
    }))
}

fn table_has_rows(db: &ZoteroDb, table: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table} LIMIT 1");
    Ok(db
        .conn()
        .query_row(&sql, [], |_| Ok(()))
        .optional()?
        .is_some())
}

fn conversation_metadata(db: &ZoteroDb) -> Result<HashMap<i64, Value>> {
    let mut map = HashMap::new();

    if db.table_exists(CLAUDE_CONVERSATIONS) {
        let mut stmt = db.conn().prepare(
            "SELECT conversation_key, library_id, kind, paper_item_id, created_at, updated_at,
                    title, provider_session_id, scoped_conversation_key, scope_type, scope_id,
                    scope_label, cwd, model_name, effort
             FROM llm_for_zotero_claude_conversations",
        )?;
        let rows = stmt.query_map([], |row| {
            let key: i64 = row.get(0)?;
            Ok((key, claude_conversation_json(db, row, key)))
        })?;
        for row in rows {
            let (key, value) = row?;
            map.insert(key, value);
        }
    }

    if db.table_exists(PAPER_CONVERSATIONS) {
        let mut stmt = db.conn().prepare(
            "SELECT conversation_key, library_id, paper_item_id, session_version, created_at, title
             FROM llm_for_zotero_paper_conversations",
        )?;
        let rows = stmt.query_map([], |row| {
            let key: i64 = row.get(0)?;
            let paper_item_id: Option<i64> = row.get(2)?;
            Ok((
                key,
                json!({
                    "conversation_key": key,
                    "conversation_system": "upstream",
                    "kind": "paper",
                    "library_id": row.get::<_, Option<i64>>(1)?,
                    "paper_item_id": paper_item_id,
                    "session_version": row.get::<_, Option<i64>>(3)?,
                    "created_at": row.get::<_, Option<i64>>(4)?,
                    "title": row.get::<_, Option<String>>(5)?,
                    "linked_item": paper_item_id.and_then(|id| db.item_summary_by_id(id).ok()),
                }),
            ))
        })?;
        for row in rows {
            let (key, value) = row?;
            map.entry(key).or_insert(value);
        }
    }

    if db.table_exists(GLOBAL_CONVERSATIONS) {
        let mut stmt = db.conn().prepare(
            "SELECT conversation_key, library_id, created_at, title
             FROM llm_for_zotero_global_conversations",
        )?;
        let rows = stmt.query_map([], |row| {
            let key: i64 = row.get(0)?;
            Ok((
                key,
                json!({
                    "conversation_key": key,
                    "conversation_system": "upstream",
                    "kind": "global",
                    "library_id": row.get::<_, Option<i64>>(1)?,
                    "created_at": row.get::<_, Option<i64>>(2)?,
                    "title": row.get::<_, Option<String>>(3)?,
                }),
            ))
        })?;
        for row in rows {
            let (key, value) = row?;
            map.entry(key).or_insert(value);
        }
    }

    Ok(map)
}

fn filter_conversations(
    conversations: HashMap<i64, Value>,
    item_id: Option<i64>,
) -> HashMap<i64, Value> {
    let Some(item_id) = item_id else {
        return conversations;
    };
    conversations
        .into_iter()
        .filter(|(_, conversation)| {
            conversation
                .get("paper_item_id")
                .and_then(Value::as_i64)
                .map(|paper_item_id| paper_item_id == item_id)
                .unwrap_or(false)
        })
        .collect()
}

fn claude_conversation_json(db: &ZoteroDb, row: &Row<'_>, key: i64) -> Value {
    let paper_item_id = row.get::<_, Option<i64>>(3).ok().flatten();
    json!({
        "conversation_key": key,
        "conversation_system": "claude_code",
        "library_id": row.get::<_, Option<i64>>(1).ok().flatten(),
        "kind": row.get::<_, Option<String>>(2).ok().flatten(),
        "paper_item_id": paper_item_id,
        "created_at": row.get::<_, Option<i64>>(4).ok().flatten(),
        "updated_at": row.get::<_, Option<i64>>(5).ok().flatten(),
        "title": row.get::<_, Option<String>>(6).ok().flatten(),
        "provider_session_id": row.get::<_, Option<String>>(7).ok().flatten(),
        "scoped_conversation_key": row.get::<_, Option<String>>(8).ok().flatten(),
        "scope_type": row.get::<_, Option<String>>(9).ok().flatten(),
        "scope_id": row.get::<_, Option<String>>(10).ok().flatten(),
        "scope_label": row.get::<_, Option<String>>(11).ok().flatten(),
        "cwd": row.get::<_, Option<String>>(12).ok().flatten(),
        "model_name": row.get::<_, Option<String>>(13).ok().flatten(),
        "effort": row.get::<_, Option<String>>(14).ok().flatten(),
        "linked_item": paper_item_id.and_then(|id| db.item_summary_by_id(id).ok()),
    })
}

fn read_messages(
    db: &ZoteroDb,
    table: &str,
    range: &DateRange,
    conversations: &HashMap<i64, Value>,
    include_contexts: bool,
    full_text: bool,
) -> Result<Vec<Value>> {
    let cols = MessageColumns::for_table(db, table, include_contexts);
    let sql = format!(
        "SELECT id, conversation_key, role, text, timestamp,
                {run_mode}, {agent_run_id}, {selected_text}, {selected_texts_json},
                {selected_text_paper_contexts_json}, {paper_contexts_json},
                {full_text_paper_contexts_json}, {model_name}, {model_entry_id},
                {model_provider_label}, {reasoning_summary}, {context_tokens}, {context_window}
         FROM {table}
         WHERE timestamp >= ? AND timestamp <= ?
         ORDER BY timestamp ASC, id ASC",
        run_mode = cols.run_mode,
        agent_run_id = cols.agent_run_id,
        selected_text = cols.selected_text,
        selected_texts_json = cols.selected_texts_json,
        selected_text_paper_contexts_json = cols.selected_text_paper_contexts_json,
        paper_contexts_json = cols.paper_contexts_json,
        full_text_paper_contexts_json = cols.full_text_paper_contexts_json,
        model_name = cols.model_name,
        model_entry_id = cols.model_entry_id,
        model_provider_label = cols.model_provider_label,
        reasoning_summary = cols.reasoning_summary,
        context_tokens = cols.context_tokens,
        context_window = cols.context_window,
    );
    let system = if table == CLAUDE_MESSAGES {
        "claude_code"
    } else {
        "upstream"
    };
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(
        params![range.from.timestamp_millis(), range.to.timestamp_millis()],
        |row| row_to_message(row, table, system, conversations, full_text),
    )?;
    let rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().flatten().collect())
}

fn read_message_by_id(
    db: &ZoteroDb,
    table: &str,
    id: i64,
    conversations: &HashMap<i64, Value>,
    include_contexts: bool,
    full_text: bool,
) -> Result<Option<Value>> {
    let cols = MessageColumns::for_table(db, table, include_contexts);
    let sql = format!(
        "SELECT id, conversation_key, role, text, timestamp,
                {run_mode}, {agent_run_id}, {selected_text}, {selected_texts_json},
                {selected_text_paper_contexts_json}, {paper_contexts_json},
                {full_text_paper_contexts_json}, {model_name}, {model_entry_id},
                {model_provider_label}, {reasoning_summary}, {context_tokens}, {context_window}
         FROM {table}
         WHERE id = ?",
        run_mode = cols.run_mode,
        agent_run_id = cols.agent_run_id,
        selected_text = cols.selected_text,
        selected_texts_json = cols.selected_texts_json,
        selected_text_paper_contexts_json = cols.selected_text_paper_contexts_json,
        paper_contexts_json = cols.paper_contexts_json,
        full_text_paper_contexts_json = cols.full_text_paper_contexts_json,
        model_name = cols.model_name,
        model_entry_id = cols.model_entry_id,
        model_provider_label = cols.model_provider_label,
        reasoning_summary = cols.reasoning_summary,
        context_tokens = cols.context_tokens,
        context_window = cols.context_window,
    );
    let system = if table == CLAUDE_MESSAGES {
        "claude_code"
    } else {
        "upstream"
    };
    let mut stmt = db.conn().prepare(&sql)?;
    stmt.query_row([id], |row| {
        row_to_message(row, table, system, conversations, full_text)
    })
    .optional()
    .map(|row| row.flatten())
    .map_err(Into::into)
}

fn row_to_message(
    row: &Row<'_>,
    table: &str,
    system: &str,
    conversations: &HashMap<i64, Value>,
    full_text: bool,
) -> rusqlite::Result<Option<Value>> {
    let id = row.get::<_, i64>(0)?;
    let conversation_key: i64 = row.get(1)?;
    if !conversations.contains_key(&conversation_key) {
        return Ok(None);
    }
    let text: String = row.get(3)?;
    let text_meta = text_meta(&text, 900, full_text);
    Ok(Some(json!({
        "id": id,
        "message_ref": message_ref(table, id),
        "turn_command": format!("zcli lfz turn {}", message_ref(table, id)),
        "conversation_key": conversation_key,
        "conversation_system": system,
        "role": row.get::<_, String>(2)?,
        "text_excerpt": text_meta["excerpt"].clone(),
        "text": text_meta["text"].clone(),
        "text_chars": text_meta["chars"].clone(),
        "text_estimated_tokens": text_meta["estimated_tokens"].clone(),
        "text_excerpt_chars": text_meta["excerpt_chars"].clone(),
        "text_excerpt_estimated_tokens": text_meta["excerpt_estimated_tokens"].clone(),
        "text_truncated": text_meta["truncated"].clone(),
        "text_full_included": full_text,
        "timestamp": row.get::<_, i64>(4)?,
        "run_mode": row.get::<_, Option<String>>(5)?,
        "agent_run_id": row.get::<_, Option<String>>(6)?,
        "selected_text": row.get::<_, Option<String>>(7)?.map(|value| excerpt(&value, 1200)),
        "selected_texts_json": row.get::<_, Option<String>>(8)?.map(|value| excerpt(&value, 3000)),
        "selected_text_paper_contexts_json": row.get::<_, Option<String>>(9)?.map(|value| excerpt(&value, 3000)),
        "paper_contexts_json": row.get::<_, Option<String>>(10)?.map(|value| excerpt(&value, 3000)),
        "full_text_paper_contexts_json": row.get::<_, Option<String>>(11)?.map(|value| excerpt(&value, 3000)),
        "model_name": row.get::<_, Option<String>>(12)?,
        "model_entry_id": row.get::<_, Option<String>>(13)?,
        "model_provider_label": row.get::<_, Option<String>>(14)?,
        "reasoning_summary": row.get::<_, Option<String>>(15)?,
        "context_tokens": row.get::<_, Option<i64>>(16)?,
        "context_window": row.get::<_, Option<i64>>(17)?,
        "conversation": conversations.get(&conversation_key),
    })))
}

fn previous_user_message(
    db: &ZoteroDb,
    table: &str,
    conversation_key: i64,
    before_ts: i64,
    conversations: &HashMap<i64, Value>,
) -> Result<Option<Value>> {
    let id = db
        .conn()
        .query_row(
            &format!(
                "SELECT id FROM {table}
                 WHERE conversation_key = ? AND role = 'user' AND timestamp <= ?
                 ORDER BY timestamp DESC, id DESC
                 LIMIT 1"
            ),
            params![conversation_key, before_ts],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    id.map(|id| read_message_by_id(db, table, id, conversations, false, true))
        .transpose()
        .map(Option::flatten)
}

fn next_user_timestamp(
    db: &ZoteroDb,
    table: &str,
    conversation_key: i64,
    after_ts: i64,
) -> Result<Option<i64>> {
    db.conn()
        .query_row(
            &format!(
                "SELECT MIN(timestamp) FROM {table}
                 WHERE conversation_key = ? AND role = 'user' AND timestamp > ?"
            ),
            params![conversation_key, after_ts],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(Into::into)
}

fn assistant_messages_after(
    db: &ZoteroDb,
    table: &str,
    conversation_key: i64,
    from_ts: i64,
    until_ts: Option<i64>,
    conversations: &HashMap<i64, Value>,
) -> Result<Vec<Value>> {
    let cols = MessageColumns::for_table(db, table, false);
    let upper = if until_ts.is_some() {
        "AND timestamp < ?"
    } else {
        ""
    };
    let sql = format!(
        "SELECT id, conversation_key, role, text, timestamp,
                {run_mode}, {agent_run_id}, {selected_text}, {selected_texts_json},
                {selected_text_paper_contexts_json}, {paper_contexts_json},
                {full_text_paper_contexts_json}, {model_name}, {model_entry_id},
                {model_provider_label}, {reasoning_summary}, {context_tokens}, {context_window}
         FROM {table}
         WHERE conversation_key = ? AND role = 'assistant' AND timestamp >= ? {upper}
         ORDER BY timestamp ASC, id ASC",
        run_mode = cols.run_mode,
        agent_run_id = cols.agent_run_id,
        selected_text = cols.selected_text,
        selected_texts_json = cols.selected_texts_json,
        selected_text_paper_contexts_json = cols.selected_text_paper_contexts_json,
        paper_contexts_json = cols.paper_contexts_json,
        full_text_paper_contexts_json = cols.full_text_paper_contexts_json,
        model_name = cols.model_name,
        model_entry_id = cols.model_entry_id,
        model_provider_label = cols.model_provider_label,
        reasoning_summary = cols.reasoning_summary,
        context_tokens = cols.context_tokens,
        context_window = cols.context_window,
    );
    let system = if table == CLAUDE_MESSAGES {
        "claude_code"
    } else {
        "upstream"
    };
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = if let Some(until_ts) = until_ts {
        stmt.query_map(params![conversation_key, from_ts, until_ts], |row| {
            row_to_message(row, table, system, conversations, true)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![conversation_key, from_ts], |row| {
            row_to_message(row, table, system, conversations, true)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    Ok(rows.into_iter().flatten().collect())
}

fn turn_agent_runs(
    db: &ZoteroDb,
    conversation_key: i64,
    from_ts: i64,
    until_ts: Option<i64>,
    question: &Value,
    answers: &[Value],
    conversations: &HashMap<i64, Value>,
) -> Result<Vec<Value>> {
    let mut run_ids = Vec::new();
    if let Some(run_id) = question.get("agent_run_id").and_then(Value::as_str) {
        run_ids.push(run_id.to_string());
    }
    for answer in answers {
        if let Some(run_id) = answer.get("agent_run_id").and_then(Value::as_str) {
            if !run_ids.iter().any(|known| known == run_id) {
                run_ids.push(run_id.to_string());
            }
        }
    }
    if run_ids.is_empty() && db.table_exists(AGENT_RUNS) {
        let upper = if until_ts.is_some() {
            "AND created_at < ?"
        } else {
            ""
        };
        let sql = format!(
            "SELECT run_id FROM {AGENT_RUNS}
             WHERE conversation_key = ? AND created_at >= ? {upper}
             ORDER BY created_at ASC"
        );
        let mut stmt = db.conn().prepare(&sql)?;
        let rows = if let Some(until_ts) = until_ts {
            stmt.query_map(params![conversation_key, from_ts, until_ts], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![conversation_key, from_ts], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        run_ids.extend(rows);
    }

    let mut runs = Vec::new();
    for run_id in run_ids {
        if let Some(run) = read_agent_run_by_id(db, &run_id, true, conversations)? {
            runs.push(run);
        }
    }
    Ok(runs)
}

fn read_agent_run_by_id(
    db: &ZoteroDb,
    run_id: &str,
    full_text: bool,
    conversations: &HashMap<i64, Value>,
) -> Result<Option<Value>> {
    if !db.table_exists(AGENT_RUNS) {
        return Ok(None);
    }
    let mut stmt = db.conn().prepare(
        "SELECT run_id, conversation_key, mode, model_name, status, created_at, completed_at, final_text
         FROM llm_for_zotero_agent_runs
         WHERE run_id = ?",
    )?;
    stmt.query_row([run_id], |row| {
        let run_id: String = row.get(0)?;
        let conversation_key: i64 = row.get(1)?;
        let final_text = row.get::<_, Option<String>>(7)?;
        let final_meta = final_text
            .as_deref()
            .map(|text| text_meta(text, 600, full_text))
            .unwrap_or_else(|| text_meta("", 600, full_text));
        Ok(json!({
            "run_id": run_id,
            "conversation_key": conversation_key,
            "mode": row.get::<_, String>(2)?,
            "model_name": row.get::<_, Option<String>>(3)?,
            "status": row.get::<_, String>(4)?,
            "created_at": row.get::<_, i64>(5)?,
            "completed_at": row.get::<_, Option<i64>>(6)?,
            "final_text_excerpt": final_meta["excerpt"].clone(),
            "final_text": final_meta["text"].clone(),
            "final_text_chars": final_meta["chars"].clone(),
            "final_text_estimated_tokens": final_meta["estimated_tokens"].clone(),
            "final_text_excerpt_chars": final_meta["excerpt_chars"].clone(),
            "final_text_excerpt_estimated_tokens": final_meta["excerpt_estimated_tokens"].clone(),
            "final_text_truncated": final_meta["truncated"].clone(),
            "final_text_full_included": full_text,
            "trace_payloads": "disabled",
            "event_count": agent_event_count(db, &run_id).unwrap_or(0),
            "conversation": conversations.get(&conversation_key),
        }))
    })
    .optional()
    .map_err(Into::into)
}

fn read_agent_runs(
    db: &ZoteroDb,
    range: &DateRange,
    full_text: bool,
    conversations: &HashMap<i64, Value>,
) -> Result<Vec<Value>> {
    if !db.table_exists(AGENT_RUNS) {
        return Ok(Vec::new());
    }
    let mut stmt = db.conn().prepare(
        "SELECT run_id, conversation_key, mode, model_name, status, created_at, completed_at, final_text
         FROM llm_for_zotero_agent_runs
         WHERE created_at <= ? AND COALESCE(completed_at, created_at) >= ?
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(
        params![range.to.timestamp_millis(), range.from.timestamp_millis()],
        |row| {
            let run_id: String = row.get(0)?;
            let conversation_key: i64 = row.get(1)?;
            if !conversations.contains_key(&conversation_key) {
                return Ok(None);
            }
            let final_text = row.get::<_, Option<String>>(7)?;
            let final_meta = final_text
                .as_deref()
                .map(|text| text_meta(text, 600, full_text))
                .unwrap_or_else(|| text_meta("", 600, full_text));
            Ok(Some(json!({
                "run_id": run_id,
                "conversation_key": conversation_key,
                "mode": row.get::<_, String>(2)?,
                "model_name": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "created_at": row.get::<_, i64>(5)?,
                "completed_at": row.get::<_, Option<i64>>(6)?,
                "final_text_excerpt": final_meta["excerpt"].clone(),
                "final_text": final_meta["text"].clone(),
                "final_text_chars": final_meta["chars"].clone(),
                "final_text_estimated_tokens": final_meta["estimated_tokens"].clone(),
                "final_text_excerpt_chars": final_meta["excerpt_chars"].clone(),
                "final_text_excerpt_estimated_tokens": final_meta["excerpt_estimated_tokens"].clone(),
                "final_text_truncated": final_meta["truncated"].clone(),
                "final_text_full_included": full_text,
                "trace_payloads": "disabled",
                "event_count": agent_event_count(db, &run_id).unwrap_or(0),
                "conversation": conversations.get(&conversation_key),
            })))
        },
    )?;
    let rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().flatten().collect())
}

fn compact_recap(detail: &Value, limit: usize) -> Value {
    let conversations = detail
        .get("conversations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let messages = detail
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let agent_runs = detail
        .get("agent_runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let user_messages = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .take(limit)
        .map(compact_message)
        .collect::<Vec<_>>();
    let assistant_messages = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
        .take(limit)
        .map(compact_message)
        .collect::<Vec<_>>();
    let finals = agent_runs
        .iter()
        .filter(|run| {
            run.get("final_text_excerpt")
                .and_then(Value::as_str)
                .map(|text| !text.is_empty())
                .unwrap_or(false)
        })
        .take(limit)
        .map(compact_run)
        .collect::<Vec<_>>();
    let paper_groups = compact_paper_groups(&conversations, &messages, &agent_runs, limit);
    let full_text = detail
        .get("full_text")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    json!({
        "status": detail.get("status").cloned().unwrap_or_else(|| json!("unavailable")),
        "runtime_dir": detail.get("runtime_dir").cloned().unwrap_or(Value::Null),
        "item_id": detail.get("item_id").cloned().unwrap_or(Value::Null),
        "compact": true,
        "limit": limit,
        "full_text": full_text,
        "text_policy": text_policy(full_text),
        "expand_policy": expand_policy(full_text),
        "paper_group_policy": "grouped_by_linked_zotero_item_when_available",
        "trace_payloads": "disabled",
        "include_contexts": detail.get("include_contexts").cloned().unwrap_or_else(|| json!(false)),
        "counts": {
            "conversations": conversations.len(),
            "messages": messages.len(),
            "user_questions": messages.iter().filter(|message| message.get("role").and_then(Value::as_str) == Some("user")).count(),
            "assistant_answers": messages.iter().filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant")).count(),
            "agent_runs": agent_runs.len(),
            "agent_finals": agent_runs.iter().filter(|run| run.get("final_text_excerpt").and_then(Value::as_str).map(|text| !text.is_empty()).unwrap_or(false)).count(),
            "event_count": agent_runs.iter().filter_map(|run| run.get("event_count").and_then(Value::as_i64)).sum::<i64>(),
        },
        "paper_groups": paper_groups,
        "conversations": conversations.iter().take(limit).map(compact_conversation).collect::<Vec<_>>(),
        "questions": user_messages,
        "answers": assistant_messages,
        "agent_finals": finals,
        "provenance": "llm_for_zotero_sqlite",
    })
}

fn compact_message(message: &Value) -> Value {
    let conversation = message.get("conversation");
    json!({
        "id": message.get("id").cloned().unwrap_or(Value::Null),
        "conversation_key": message.get("conversation_key").cloned().unwrap_or(Value::Null),
        "conversation_system": message.get("conversation_system").cloned().unwrap_or(Value::Null),
        "role": message.get("role").cloned().unwrap_or(Value::Null),
        "timestamp": message.get("timestamp").cloned().unwrap_or(Value::Null),
        "message_ref": message.get("message_ref").cloned().unwrap_or(Value::Null),
        "turn_command": message.get("turn_command").cloned().unwrap_or(Value::Null),
        "expand_command": message.get("turn_command").cloned().unwrap_or(Value::Null),
        "text_excerpt": message.get("text_excerpt").cloned().unwrap_or(Value::Null),
        "text": message.get("text").cloned().unwrap_or(Value::Null),
        "text_chars": message.get("text_chars").cloned().unwrap_or(Value::Null),
        "text_estimated_tokens": message.get("text_estimated_tokens").cloned().unwrap_or(Value::Null),
        "text_excerpt_chars": message.get("text_excerpt_chars").cloned().unwrap_or(Value::Null),
        "text_excerpt_estimated_tokens": message.get("text_excerpt_estimated_tokens").cloned().unwrap_or(Value::Null),
        "text_truncated": message.get("text_truncated").cloned().unwrap_or(Value::Null),
        "text_full_included": message.get("text_full_included").cloned().unwrap_or(Value::Null),
        "full_text_available": true,
        "run_mode": message.get("run_mode").cloned().unwrap_or(Value::Null),
        "agent_run_id": message.get("agent_run_id").cloned().unwrap_or(Value::Null),
        "model_name": message.get("model_name").cloned().unwrap_or(Value::Null),
        "model_provider_label": message.get("model_provider_label").cloned().unwrap_or(Value::Null),
        "linked_item": conversation.and_then(|value| value.get("linked_item")).map(compact_item).unwrap_or(Value::Null),
    })
}

fn compact_run(run: &Value) -> Value {
    let conversation = run.get("conversation");
    json!({
        "run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
        "conversation_key": run.get("conversation_key").cloned().unwrap_or(Value::Null),
        "mode": run.get("mode").cloned().unwrap_or(Value::Null),
        "model_name": run.get("model_name").cloned().unwrap_or(Value::Null),
        "status": run.get("status").cloned().unwrap_or(Value::Null),
        "created_at": run.get("created_at").cloned().unwrap_or(Value::Null),
        "completed_at": run.get("completed_at").cloned().unwrap_or(Value::Null),
        "final_text_excerpt": run.get("final_text_excerpt").cloned().unwrap_or(Value::Null),
        "final_text": run.get("final_text").cloned().unwrap_or(Value::Null),
        "final_text_chars": run.get("final_text_chars").cloned().unwrap_or(Value::Null),
        "final_text_estimated_tokens": run.get("final_text_estimated_tokens").cloned().unwrap_or(Value::Null),
        "final_text_excerpt_chars": run.get("final_text_excerpt_chars").cloned().unwrap_or(Value::Null),
        "final_text_excerpt_estimated_tokens": run.get("final_text_excerpt_estimated_tokens").cloned().unwrap_or(Value::Null),
        "final_text_truncated": run.get("final_text_truncated").cloned().unwrap_or(Value::Null),
        "final_text_full_included": run.get("final_text_full_included").cloned().unwrap_or(Value::Null),
        "full_text_available": true,
        "event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
        "linked_item": conversation.and_then(|value| value.get("linked_item")).map(compact_item).unwrap_or(Value::Null),
    })
}

#[derive(Default)]
struct PaperGroup {
    group_key: String,
    linked_item: Value,
    conversation_keys: HashSet<i64>,
    message_count: usize,
    question_count: usize,
    answer_count: usize,
    agent_run_count: usize,
    final_count: usize,
    event_count: i64,
    sample_questions: Vec<Value>,
    sample_answers: Vec<Value>,
    sample_finals: Vec<Value>,
    expand_commands: Vec<String>,
}

fn compact_paper_groups(
    _conversations: &[Value],
    messages: &[Value],
    agent_runs: &[Value],
    limit: usize,
) -> Vec<Value> {
    let mut groups: HashMap<String, PaperGroup> = HashMap::new();

    for message in messages {
        let conversation = message.get("conversation").unwrap_or(&Value::Null);
        let key = paper_group_key(conversation);
        let group = groups.entry(key.clone()).or_insert_with(|| PaperGroup {
            group_key: key,
            linked_item: conversation
                .get("linked_item")
                .map(compact_item)
                .unwrap_or(Value::Null),
            ..PaperGroup::default()
        });
        if let Some(conversation_key) = message.get("conversation_key").and_then(Value::as_i64) {
            group.conversation_keys.insert(conversation_key);
        }
        group.message_count += 1;
        match message.get("role").and_then(Value::as_str) {
            Some("user") => {
                group.question_count += 1;
                push_limited(
                    &mut group.sample_questions,
                    group_message_sample(message),
                    2,
                );
                if let Some(command) = message.get("turn_command").and_then(Value::as_str) {
                    push_unique_limited(&mut group.expand_commands, command.to_string(), 5);
                }
            }
            Some("assistant") => {
                group.answer_count += 1;
                push_limited(&mut group.sample_answers, group_message_sample(message), 2);
            }
            _ => {}
        }
    }

    for run in agent_runs {
        let conversation = run.get("conversation").unwrap_or(&Value::Null);
        let key = paper_group_key(conversation);
        let group = groups.entry(key.clone()).or_insert_with(|| PaperGroup {
            group_key: key,
            linked_item: conversation
                .get("linked_item")
                .map(compact_item)
                .unwrap_or(Value::Null),
            ..PaperGroup::default()
        });
        if let Some(conversation_key) = run.get("conversation_key").and_then(Value::as_i64) {
            group.conversation_keys.insert(conversation_key);
        }
        group.agent_run_count += 1;
        group.event_count += run.get("event_count").and_then(Value::as_i64).unwrap_or(0);
        if run
            .get("final_text_excerpt")
            .and_then(Value::as_str)
            .map(|text| !text.is_empty())
            .unwrap_or(false)
        {
            group.final_count += 1;
            push_limited(&mut group.sample_finals, group_run_sample(run), 2);
        }
    }

    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by(|left, right| {
        let left_score = left.message_count + left.agent_run_count;
        let right_score = right.message_count + right.agent_run_count;
        right_score
            .cmp(&left_score)
            .then_with(|| left.group_key.cmp(&right.group_key))
    });

    groups
        .into_iter()
        .take(limit)
        .map(|group| {
            let mut conversation_keys = group.conversation_keys.into_iter().collect::<Vec<_>>();
            conversation_keys.sort_unstable();
            let conversation_count = conversation_keys.len();
            json!({
                "group_key": group.group_key,
                "linked_item": group.linked_item,
                "conversation_keys": conversation_keys,
                "counts": {
                    "conversations": conversation_count,
                    "messages": group.message_count,
                    "questions": group.question_count,
                    "answers": group.answer_count,
                    "agent_runs": group.agent_run_count,
                    "agent_finals": group.final_count,
                    "event_count": group.event_count,
                },
                "sample_questions": group.sample_questions,
                "sample_answers": group.sample_answers,
                "sample_finals": group.sample_finals,
                "expand_commands": group.expand_commands,
                "text_policy": "compact_index",
            })
        })
        .collect()
}

fn group_message_sample(message: &Value) -> Value {
    let excerpt_value = message
        .get("text_excerpt")
        .and_then(Value::as_str)
        .map(|text| excerpt(text, 240))
        .unwrap_or_default();
    json!({
        "message_ref": message.get("message_ref").cloned().unwrap_or(Value::Null),
        "turn_command": message.get("turn_command").cloned().unwrap_or(Value::Null),
        "role": message.get("role").cloned().unwrap_or(Value::Null),
        "timestamp": message.get("timestamp").cloned().unwrap_or(Value::Null),
        "text_excerpt": excerpt_value,
        "text_chars": message.get("text_chars").cloned().unwrap_or(Value::Null),
        "text_truncated": message.get("text_truncated").cloned().unwrap_or(Value::Null),
    })
}

fn group_run_sample(run: &Value) -> Value {
    let excerpt_value = run
        .get("final_text_excerpt")
        .and_then(Value::as_str)
        .map(|text| excerpt(text, 240))
        .unwrap_or_default();
    json!({
        "run_id": run.get("run_id").cloned().unwrap_or(Value::Null),
        "model_name": run.get("model_name").cloned().unwrap_or(Value::Null),
        "created_at": run.get("created_at").cloned().unwrap_or(Value::Null),
        "completed_at": run.get("completed_at").cloned().unwrap_or(Value::Null),
        "final_text_excerpt": excerpt_value,
        "final_text_chars": run.get("final_text_chars").cloned().unwrap_or(Value::Null),
        "final_text_truncated": run.get("final_text_truncated").cloned().unwrap_or(Value::Null),
        "event_count": run.get("event_count").cloned().unwrap_or(Value::Null),
    })
}

fn paper_group_key(conversation: &Value) -> String {
    if let Some(key) = conversation
        .get("linked_item")
        .and_then(|item| item.get("key"))
        .and_then(Value::as_str)
    {
        return format!("item:{key}");
    }
    if let Some(id) = conversation
        .get("linked_item")
        .and_then(|item| item.get("id"))
        .and_then(Value::as_i64)
    {
        return format!("item_id:{id}");
    }
    if let Some(conversation_key) = conversation.get("conversation_key").and_then(Value::as_i64) {
        return format!("conversation:{conversation_key}");
    }
    "unlinked".to_string()
}

fn push_limited(values: &mut Vec<Value>, value: Value, limit: usize) {
    if values.len() < limit {
        values.push(value);
    }
}

fn push_unique_limited(values: &mut Vec<String>, value: String, limit: usize) {
    if values.len() < limit && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn compact_conversation(conversation: &Value) -> Value {
    json!({
        "conversation_key": conversation.get("conversation_key").cloned().unwrap_or(Value::Null),
        "conversation_system": conversation.get("conversation_system").cloned().unwrap_or(Value::Null),
        "kind": conversation.get("kind").cloned().unwrap_or(Value::Null),
        "title": conversation.get("title").cloned().unwrap_or(Value::Null),
        "scope_type": conversation.get("scope_type").cloned().unwrap_or(Value::Null),
        "scope_label": conversation.get("scope_label").cloned().unwrap_or(Value::Null),
        "model_name": conversation.get("model_name").cloned().unwrap_or(Value::Null),
        "updated_at": conversation.get("updated_at").cloned().unwrap_or(Value::Null),
        "linked_item": conversation.get("linked_item").map(compact_item).unwrap_or(Value::Null),
    })
}

fn compact_item(item: &Value) -> Value {
    json!({
        "id": item.get("id").cloned().unwrap_or(Value::Null),
        "key": item.get("key").cloned().unwrap_or(Value::Null),
        "citation_key": item.get("citation_key").cloned().unwrap_or(Value::Null),
        "title": item.get("title").cloned().unwrap_or(Value::Null),
        "short_title": item.get("short_title").cloned().unwrap_or(Value::Null),
        "year": item.get("year").cloned().unwrap_or(Value::Null),
        "doi": item.get("doi").cloned().unwrap_or(Value::Null),
        "arxiv": item.get("arxiv").cloned().unwrap_or(Value::Null),
        "url": item.get("url").cloned().unwrap_or(Value::Null),
    })
}

fn agent_event_count(db: &ZoteroDb, run_id: &str) -> Result<i64> {
    if !db.table_exists(AGENT_EVENTS) {
        return Ok(0);
    }
    db.conn()
        .query_row(
            "SELECT COUNT(*) FROM llm_for_zotero_agent_run_events WHERE run_id = ?",
            [run_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

struct MessageColumns {
    run_mode: String,
    agent_run_id: String,
    selected_text: String,
    selected_texts_json: String,
    selected_text_paper_contexts_json: String,
    paper_contexts_json: String,
    full_text_paper_contexts_json: String,
    model_name: String,
    model_entry_id: String,
    model_provider_label: String,
    reasoning_summary: String,
    context_tokens: String,
    context_window: String,
}

impl MessageColumns {
    fn for_table(db: &ZoteroDb, table: &str, include_contexts: bool) -> Self {
        Self {
            run_mode: maybe_col(db, table, "run_mode"),
            agent_run_id: maybe_col(db, table, "agent_run_id"),
            selected_text: context_col(db, table, "selected_text", include_contexts),
            selected_texts_json: context_col(db, table, "selected_texts_json", include_contexts),
            selected_text_paper_contexts_json: context_col(
                db,
                table,
                "selected_text_paper_contexts_json",
                include_contexts,
            ),
            paper_contexts_json: context_col(db, table, "paper_contexts_json", include_contexts),
            full_text_paper_contexts_json: context_col(
                db,
                table,
                "full_text_paper_contexts_json",
                include_contexts,
            ),
            model_name: maybe_col(db, table, "model_name"),
            model_entry_id: maybe_col(db, table, "model_entry_id"),
            model_provider_label: maybe_col(db, table, "model_provider_label"),
            reasoning_summary: maybe_col(db, table, "reasoning_summary"),
            context_tokens: maybe_col(db, table, "context_tokens"),
            context_window: maybe_col(db, table, "context_window"),
        }
    }
}

fn context_col(db: &ZoteroDb, table: &str, column: &str, include_contexts: bool) -> String {
    if include_contexts {
        maybe_col(db, table, column)
    } else {
        "NULL".to_string()
    }
}

fn maybe_col(db: &ZoteroDb, table: &str, column: &str) -> String {
    if db.column_exists(table, column) {
        column.to_string()
    } else {
        "NULL".to_string()
    }
}

fn parse_message_ref(message_ref: &str) -> Result<(&'static str, i64)> {
    let (prefix, id) = message_ref
        .split_once(':')
        .ok_or_else(|| anyhow!("message ref must look like claude:123 or upstream:123"))?;
    let table = match prefix {
        "claude" | "cc" | CLAUDE_MESSAGES => CLAUDE_MESSAGES,
        "upstream" | UPSTREAM_MESSAGES => UPSTREAM_MESSAGES,
        "legacy" | LEGACY_MESSAGES => LEGACY_MESSAGES,
        _ => anyhow::bail!("unknown llm-for-zotero message ref prefix: {prefix}"),
    };
    let id = id
        .parse::<i64>()
        .map_err(|_| anyhow!("message ref id must be an integer: {message_ref}"))?;
    Ok((table, id))
}

fn message_ref(table: &str, id: i64) -> String {
    format!("{}:{id}", table_ref_prefix(table))
}

fn table_ref_prefix(table: &str) -> &'static str {
    match table {
        CLAUDE_MESSAGES => "claude",
        LEGACY_MESSAGES => "legacy",
        _ => "upstream",
    }
}

fn text_meta(value: &str, max_chars: usize, full_text: bool) -> Value {
    let normalized = normalized_text(value);
    let excerpt = excerpt_normalized(&normalized, max_chars);
    let chars = normalized.chars().count();
    let excerpt_chars = excerpt.chars().count();
    json!({
        "excerpt": excerpt,
        "text": if full_text { Value::String(normalized) } else { Value::Null },
        "chars": chars,
        "estimated_tokens": estimated_tokens(chars),
        "excerpt_chars": excerpt_chars,
        "excerpt_estimated_tokens": estimated_tokens(excerpt_chars),
        "truncated": !full_text && chars > excerpt_chars,
        "excerpt_truncated": chars > excerpt_chars,
    })
}

fn text_policy(full_text: bool) -> &'static str {
    if full_text {
        "full_text_requested"
    } else {
        "compact_index"
    }
}

fn expand_policy(full_text: bool) -> &'static str {
    if full_text {
        "full_text_is_included_without_trace_payloads"
    } else {
        "use_turn_command_or_expand_command_for_one_specific_full_question_and_final_answer"
    }
}

fn estimated_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

fn excerpt(value: &str, max_chars: usize) -> String {
    excerpt_normalized(&normalized_text(value), max_chars)
}

fn normalized_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn excerpt_normalized(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::text_meta;

    #[test]
    fn text_meta_marks_full_text_as_not_truncated() {
        let value = "abcdef";
        let compact = text_meta(value, 3, false);
        assert_eq!(compact["truncated"], true);
        assert_eq!(compact["excerpt_truncated"], true);
        assert_eq!(compact["text"], serde_json::Value::Null);

        let full = text_meta(value, 3, true);
        assert_eq!(full["truncated"], false);
        assert_eq!(full["excerpt_truncated"], true);
        assert_eq!(full["text"], value);
    }
}
