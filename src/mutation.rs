use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::{config::Config, helper, local_api};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationTransport {
    LocalApi,
    Helper,
}

pub struct MutationPlan {
    pub op: &'static str,
    pub transport: MutationTransport,
    pub params: Value,
    pub preview: Value,
    pub empty_execute_reason: Option<&'static str>,
}

impl MutationPlan {
    pub fn local_api(op: &'static str, params: Value, preview: Value) -> Self {
        Self {
            op,
            transport: MutationTransport::LocalApi,
            params,
            preview,
            empty_execute_reason: None,
        }
    }

    pub fn helper(op: &'static str, params: Value, preview: Value) -> Self {
        Self {
            op,
            transport: MutationTransport::Helper,
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
            "write commands are dry-run-first; pass --dry-run to preview or --execute"
        ));
    }
    Ok(())
}

pub fn preview_or_execute(config: &Config, dry_run: bool, plan: MutationPlan) -> Result<Value> {
    if dry_run {
        let transport = transport_name(plan.transport);
        return Ok(json!({
            "ok": true,
            "dry_run": true,
            "transport": transport,
            "operation": plan.op,
            "local_api_required_for_execute": matches!(plan.transport, MutationTransport::LocalApi),
            "helper_required_for_execute": matches!(plan.transport, MutationTransport::Helper),
            "helper_op": matches!(plan.transport, MutationTransport::Helper).then_some(plan.op),
            "params": plan.params,
            "preview": plan.preview,
        }));
    }
    if let Some(reason) = plan.empty_execute_reason {
        if payload_is_empty(&plan.params) {
            return Ok(json!({
                "ok": true,
                "dry_run": false,
                "transport": transport_name(plan.transport),
                "operation": plan.op,
                "executed": false,
                "reason": reason,
                "params": plan.params,
                "preview": plan.preview,
            }));
        }
    }
    let result = match plan.transport {
        MutationTransport::LocalApi => local_api::call(config, plan.op, plan.params.clone())?,
        MutationTransport::Helper => helper::call(config, plan.op, plan.params.clone())?,
    };
    Ok(json!({
        "ok": true,
        "dry_run": false,
        "transport": transport_name(plan.transport),
        "operation": plan.op,
        "helper_op": matches!(plan.transport, MutationTransport::Helper).then_some(plan.op),
        "params": plan.params,
        "result": result,
    }))
}

fn transport_name(transport: MutationTransport) -> &'static str {
    match transport {
        MutationTransport::LocalApi => "local_api",
        MutationTransport::Helper => "helper",
    }
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
            MutationPlan::helper("op", json!({"x": 1}), json!({"target": "paper"})),
        )
        .unwrap();
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["operation"], "op");
        assert_eq!(value["transport"], "helper");
        assert_eq!(value["preview"]["target"], "paper");
    }
}
