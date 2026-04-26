use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::date_range::DateRange;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityRecord {
    pub ts: i64,
    pub source: String,
    pub command: String,
    pub item_key: Option<String>,
    pub item_id: Option<i64>,
    pub title: Option<String>,
}

pub fn log_path(state_dir: Option<&Path>) -> Option<PathBuf> {
    state_dir.map(|dir| dir.join("activity.jsonl"))
}

pub fn append_read(
    state_dir: Option<&Path>,
    command: &str,
    item_key: Option<&str>,
    item_id: Option<i64>,
    title: Option<&str>,
) {
    let Some(path) = log_path(state_dir) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let record = ActivityRecord {
        ts: Utc::now().timestamp_millis(),
        source: "cli_read_log".to_string(),
        command: command.to_string(),
        item_key: item_key.map(ToOwned::to_owned),
        item_id,
        title: title.map(ToOwned::to_owned),
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        if let Ok(line) = serde_json::to_string(&record) {
            let _ = writeln!(file, "{line}");
        }
    }
}

pub fn read_records(state_dir: Option<&Path>, range: &DateRange) -> Result<Vec<ActivityRecord>> {
    let Some(path) = log_path(state_dir) else {
        return Ok(Vec::new());
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<ActivityRecord>(&line) {
            if range.contains_millis(record.ts) {
                records.push(record);
            }
        }
    }
    Ok(records)
}
