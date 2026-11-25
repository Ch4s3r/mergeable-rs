use crate::configuration;
use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::api;
use crate::json_value;
use crate::metadata;
use crate::registry::Registry;
use crate::workflow;
use futures::future::join_all;
use serde_json::Value;

pub async fn run_all(context: &mut Context) -> Result<()> {
    check_rerun(context).await?;
    milestoned(context).await?;
    push(context).await
}

async fn check_rerun(context: &mut Context) -> Result<()> {
    if !(context.event_name == "check_run"
        && json_value::as_str(context.action()) == Some("rerequested"))
    {
        return Ok(());
    }
    let Some(check_run) = context
        .payload
        .get("check_run")
        .filter(|check_run| json_value::is_truthy(check_run))
    else {
        return Ok(());
    };
    if !json_value::truthy(check_run.get("output")) {
        return Ok(());
    }
    let rerun_metadata = match json_value::read_path(check_run, &["output", "text"])? {
        Some(Value::String(text)) => metadata::deserialize(text)?,
        Some(other) => metadata::deserialize(&json_value::to_display_string(Some(other)))?,
        None => {
            return Err(Error::missing_property("indexOf"));
        }
    };
    if is_possible_injection(check_run, &rerun_metadata) {
        return Ok(());
    }
    let first_pull_request = match json_value::read(Some(check_run), "pull_requests")? {
        Some(Value::Array(items)) => match items.first() {
            Some(first) => first,
            None => return Ok(()),
        },
        _ => {
            return Err(Error::missing_property("length"));
        }
    };
    let pull_number = json_value::read(Some(first_pull_request), "number")?.unwrap_or(&Value::Null);
    let pull_request = api::fetch_pull_request(context, pull_number).await?;
    if let Value::Object(payload) = context.payload.to_mut() {
        match rerun_metadata.get("action") {
            Some(action) => payload.insert("action".to_string(), action.clone()),
            None => payload.shift_remove("action"),
        };
        payload.insert("pull_request".to_string(), pull_request.data);
    }
    context.event_name = json_value::to_display_string(rerun_metadata.get("eventName"));
    Ok(())
}

fn is_possible_injection(check_run: &Value, rerun_metadata: &Value) -> bool {
    let check_run_id = check_run.get("id").unwrap_or(&Value::Null);
    let metadata_id = rerun_metadata.get("id").unwrap_or(&Value::Null);
    let ids_differ = !json_value::strict_equals(check_run_id, metadata_id);
    if ids_differ {
        tracing::warn!(
            check_run = %check_run,
            metadata = %rerun_metadata,
            "check_run.rerun.id_mismatch"
        );
    }
    ids_differ
}

fn is_milestoned_pull_request(event_name: &str, payload: &Value) -> Result<bool> {
    let is_milestone_action = matches!(
        json_value::as_str(payload.get("action")),
        Some("milestoned" | "demilestoned")
    );
    if !(event_name == "issues" && is_milestone_action) {
        return Ok(false);
    }
    Ok(json_value::truthy(json_value::read_path(
        payload,
        &["issue", "pull_request"],
    )?))
}

async fn milestoned(context: &mut Context) -> Result<()> {
    if !is_milestoned_pull_request(&context.event_name, &context.payload)? {
        return Ok(());
    }
    let issue_number =
        json_value::read_path(&context.payload, &["issue", "number"])?.unwrap_or(&Value::Null);
    let response = api::fetch_pull_request(context, issue_number).await?;
    let mut pull_request = response.data;
    if let Value::Object(fields) = &mut pull_request {
        fields.insert(
            "action".to_string(),
            context.action().cloned().unwrap_or(Value::Null),
        );
    }
    context.event_name = "pull_request".to_string();
    if let Value::Object(payload) = context.payload.to_mut() {
        payload.insert("pull_request".to_string(), pull_request);
    }
    Ok(())
}

async fn push(context: &mut Context) -> Result<()> {
    if context.event_name != "push" {
        return Ok(());
    }
    let Some(head_commit) = context
        .payload
        .get("head_commit")
        .filter(|commit| json_value::is_truthy(commit))
    else {
        return Ok(());
    };
    let config_file = Value::String(format!(
        ".github/{}",
        context
            .app
            .behavior
            .settings_file_defaults
            .config_file_name()
    ));
    let changed = json_value::includes(head_commit.get("added"), &config_file)?
        || json_value::includes(head_commit.get("modified"), &config_file)?;
    if !changed {
        return Ok(());
    }
    let configuration = configuration::load(context).await;
    if configuration.has_errors() {
        workflow::report_configuration_errors(context, &configuration).await?;
        return Ok(());
    }
    let registry = Registry::default();
    let response = api::list_pull_requests(context).await?;
    let pulls = match response.data {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let pending = pulls.into_iter().map(|pull_request| {
        let mut rerun_context = context.clone();
        rerun_context.event_name = "pull_request".to_string();
        if let Value::Object(payload) = rerun_context.payload.to_mut() {
            payload.insert("pull_request".to_string(), pull_request);
            payload.insert(
                "action".to_string(),
                Value::String("push_synchronize".to_string()),
            );
        }
        let registry = &registry;
        let configuration = &configuration;
        async move { workflow::process(&rerun_context, registry, configuration).await }
    });
    join_all(pending)
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn differing_check_run_and_metadata_ids_are_a_possible_injection() {
        assert!(!is_possible_injection(&json!({"id": 1}), &json!({"id": 1})));
        assert!(is_possible_injection(&json!({"id": 1}), &json!({"id": 2})));
    }

    #[test]
    fn only_milestone_actions_on_pull_request_issues_are_milestoned_pull_requests() {
        let mut payload = json!({"action": "opened", "repository": {"name": "repoName", "owner": {"login": "owner"}}});
        assert!(!is_milestoned_pull_request("pull_request", &payload).unwrap());
        assert!(!is_milestoned_pull_request("issues", &payload).unwrap());

        payload["issue"] = json!({});
        payload["action"] = json!("milestoned");
        assert!(!is_milestoned_pull_request("issues", &payload).unwrap());

        payload["issue"]["pull_request"] = json!({});
        assert!(is_milestoned_pull_request("issues", &payload).unwrap());

        payload["action"] = json!("demilestoned");
        assert!(is_milestoned_pull_request("issues", &payload).unwrap());

        payload["action"] = json!("edited");
        assert!(!is_milestoned_pull_request("issues", &payload).unwrap());
    }
}
