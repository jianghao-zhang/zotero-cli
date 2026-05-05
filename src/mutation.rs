use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::{config::Config, helper};

pub struct MutationPlan {
    pub helper_op: &'static str,
    pub params: Value,
    pub preview: Value,
    pub empty_execute_reason: Option<&'static str>,
}

impl MutationPlan {
    pub fn new(helper_op: &'static str, params: Value, preview: Value) -> Self {
        Self {
            helper_op,
            params,
            preview,
            empty_execute_reason: None,
        }
    }

    pub fn with_empty_execute_reason(mut self, reason: &'static str) -> Self {
        self.empty_execute_reason = Some(reason);
        self
    }
}

pub fn require_intent(dry_run: bool, execute: bool) -> Result<()> {
    if dry_run && execute {
        return Err(anyhow!("--dry-run and --execute cannot be used together"));
    }
    if !dry_run && !execute {
        return Err(anyhow!(
            "write commands are dry-run-first; pass --dry-run to preview or --execute to run through the optional Zotero helper"
        ));
    }
    Ok(())
}

pub fn preview_or_execute(config: &Config, dry_run: bool, plan: MutationPlan) -> Result<Value> {
    if dry_run {
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "helper_required_for_execute": true,
            "helper_op": plan.helper_op,
            "params": plan.params,
            "preview": plan.preview,
        }));
    }
    if let Some(reason) = plan.empty_execute_reason {
        if payload_is_empty(&plan.params) {
            return Ok(json!({
                "ok": true,
                "dry_run": false,
                "helper_op": plan.helper_op,
                "executed": false,
                "reason": reason,
                "params": plan.params,
                "preview": plan.preview,
            }));
        }
    }
    let result = helper::call(config, plan.helper_op, plan.params.clone())?;
    Ok(json!({
        "ok": true,
        "dry_run": false,
        "helper_op": plan.helper_op,
        "params": plan.params,
        "result": result,
    }))
}

fn payload_is_empty(params: &Value) -> bool {
    params
        .get("identifiers")
        .or_else(|| params.get("sources"))
        .or_else(|| params.get("urls"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_wraps_mutation_plan_without_helper() {
        let config = Config::default();
        let value = preview_or_execute(
            &config,
            true,
            MutationPlan::new("op", json!({"x": 1}), json!({"target": "paper"})),
        )
        .unwrap();
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["helper_op"], "op");
        assert_eq!(value["preview"]["target"], "paper");
    }
}
