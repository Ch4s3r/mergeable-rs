use crate::actions::{Action, is_comment_on_plain_issue};
use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::metadata;
use crate::template;
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

pub const SUPPORTED_EVENTS: [&str; 25] = [
    "pull_request.assigned",
    "pull_request.auto_merge_disabled",
    "pull_request.auto_merge_enabled",
    "pull_request.converted_to_draft",
    "pull_request.demilestoned",
    "pull_request.dequeued",
    "pull_request.edited",
    "pull_request.enqueued",
    "pull_request.labeled",
    "pull_request.locked",
    "pull_request.milestoned",
    "pull_request.opened",
    "pull_request.push_synchronize",
    "pull_request.ready_for_review",
    "pull_request.reopened",
    "pull_request.review_request_removed",
    "pull_request.review_requested",
    "pull_request.synchronize",
    "pull_request.unassigned",
    "pull_request.unlabeled",
    "pull_request.unlocked",
    "pull_request_review.dismissed",
    "pull_request_review.edited",
    "pull_request_review.submitted",
    "issue_comment.*",
];

#[must_use]
pub fn check_name(name: Option<&Value>) -> String {
    match name {
        None => "Mergeable".to_string(),
        Some(present) => format!(
            "Mergeable: {}",
            json_value::to_display_string(Some(present))
        ),
    }
}

#[must_use]
pub fn current_timestamp() -> String {
    format!("{:.3}", jiff::Timestamp::now())
}

#[must_use]
pub fn is_supported(event_name: &str) -> bool {
    event_aware::is_event_supported(&SUPPORTED_EVENTS, event_name)
}

fn rule_name_key(rule_name: Option<&Value>) -> Option<String> {
    rule_name.map(json_value::stringify)
}

pub async fn create_checks(context: &Context, payload: Value) -> Result<Value> {
    let mut params = match payload {
        Value::Object(fields) => fields,
        _ => Map::new(),
    };
    let check_name = check_name(params.get("name"));
    params.insert("name".to_string(), Value::String(check_name));
    let (head_branch, head_sha) = if json_value::truthy(context.payload.get("check_suite")) {
        let suite = &context.payload["check_suite"];
        (
            suite.get("head_branch").cloned(),
            suite.get("head_sha").cloned(),
        )
    } else if context.event_name == "issue_comment" {
        let issue_number =
            json_value::read_path(&context.payload, &["issue", "number"])?.unwrap_or(&Value::Null);
        let mut pull_request = api::fetch_pull_request(context, issue_number).await?.data;
        (
            json_value::take_path(&mut pull_request, &["head", "ref"])?,
            json_value::take_path(&mut pull_request, &["head", "sha"])?,
        )
    } else {
        (
            json_value::read_path(&context.payload, &["pull_request", "head", "ref"])?.cloned(),
            json_value::read_path(&context.payload, &["pull_request", "head", "sha"])?.cloned(),
        )
    };
    insert_defined(&mut params, "head_branch", head_branch);
    insert_defined(&mut params, "head_sha", head_sha);
    let response = api::create_checks(context, Value::Object(params)).await?;
    let mut result = Map::new();
    result.insert("status".to_string(), Value::from(response.status));
    result.insert("data".to_string(), response.data);
    Ok(Value::Object(result))
}

fn insert_defined(params: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    match value {
        Some(present) => {
            params.insert(key.to_string(), present);
        }
        None => {
            params.shift_remove(key);
        }
    }
}

pub async fn run(context: &Context, payload: Value) -> Result<()> {
    if is_comment_on_plain_issue(context) {
        return Ok(());
    }
    create_checks(context, payload).await.map(|_| ())
}

async fn update_checks(
    context: &Context,
    check_run_id: Value,
    name: String,
    state: Option<&Value>,
    status: Option<&Value>,
    output: Option<Value>,
) -> Result<()> {
    let output = output
        .unwrap_or_else(|| json!({"title": "Test SUCCESS output", "summary": "Success summary"}));
    let state = if json_value::truthy(state) {
        state.cloned().unwrap_or(Value::Null)
    } else {
        Value::String("completed".to_string())
    };
    let conclusion = if json_value::truthy(status) {
        status.cloned().unwrap_or(Value::Null)
    } else {
        Value::String("success".to_string())
    };
    let is_completed = state.as_str() == Some("completed");
    let mut check = Map::new();
    check.insert("name".to_string(), Value::String(name));
    check.insert("status".to_string(), state);
    check.insert("output".to_string(), output);
    check.insert("check_run_id".to_string(), check_run_id);
    if is_completed {
        check.insert("conclusion".to_string(), conclusion);
        check.insert(
            "completed_at".to_string(),
            Value::String(current_timestamp()),
        );
    }
    let params = context.repo_params(Value::Object(check))?;
    api::update_checks(context, params).await.map(|_| ())
}

#[derive(Default)]
pub struct Checks {
    check_runs_by_rule_name: Mutex<HashMap<Option<String>, Value>>,
}

impl Checks {
    fn populate_payload_with_result(
        payload_settings: &Value,
        validation_report: &Value,
        context: &Context,
    ) -> Result<Value> {
        let mut output = Map::new();
        let payload = event_aware::payload(context);
        let event = event_aware::event_attributes(context);
        let keys = json_value::object_keys(Some(payload_settings))?;
        let template_data = template::template_data(payload, validation_report);
        for key in keys {
            let template_value = json_value::get(payload_settings, &key).unwrap_or(&Value::Null);
            let rendered = template::populate_template_with_data(
                template_value,
                &template_data,
                payload,
                &event,
            )?;
            output.insert(key, Value::String(rendered));
        }
        Ok(Value::Object(output))
    }
}

#[async_trait]
impl Action for Checks {
    fn name(&self) -> &'static str {
        "checks"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &SUPPORTED_EVENTS
    }

    async fn before_validate(
        &self,
        context: &Context,
        _rule: &Value,
        rule_name: Option<&Value>,
    ) -> Result<()> {
        if is_comment_on_plain_issue(context) {
            return Ok(());
        }
        let mut payload = Map::new();
        payload.insert("status".to_string(), json!("in_progress"));
        payload.insert(
            "output".to_string(),
            json!({
                "title": "Mergeable is running.",
                "summary": "Please be patient. We'll get you the results as soon as possible."
            }),
        );
        if let Some(name) = rule_name {
            payload.insert("name".to_string(), name.clone());
        }
        payload.insert("started_at".to_string(), Value::String(current_timestamp()));
        let result = create_checks(context, Value::Object(payload)).await?;
        self.check_runs_by_rule_name
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(rule_name_key(rule_name), result);
        Ok(())
    }

    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        rule_name: Option<&Value>,
        validation_report: &Value,
    ) -> Result<()> {
        if is_comment_on_plain_issue(context) {
            return Ok(());
        }
        let check_run_id_outcome = {
            let check_runs = self
                .check_runs_by_rule_name
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let check_run_result = check_runs.get(&rule_name_key(rule_name));
            json_value::read(check_run_result, "data")
                .and_then(|data| json_value::read(data, "id"))
                .map(|id| id.cloned().unwrap_or(Value::Null))
        };
        let mut payload = match settings
            .get("payload")
            .filter(|payload| json_value::is_truthy(payload))
        {
            Some(payload_settings) => Some(Self::populate_payload_with_result(
                payload_settings,
                validation_report,
                context,
            )?),
            None => None,
        };
        let check_run_id = check_run_id_outcome?;
        if let Some(Value::Object(fields)) = payload.as_mut()
            && let Some(Value::String(text)) = fields.get_mut("text")
        {
            let meta = json!({
                "id": check_run_id,
                "eventName": context.event_name,
                "action": context.action().unwrap_or(&Value::Null),
            });
            text.push_str(&metadata::serialize(&meta));
        }
        update_checks(
            context,
            check_run_id,
            check_name(rule_name),
            settings.get("state"),
            settings.get("status"),
            payload,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_timestamp_is_utc_with_milliseconds() {
        let timestamp = current_timestamp();
        let pattern = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$").unwrap();
        assert!(pattern.is_match(&timestamp), "{timestamp}");
    }
}
