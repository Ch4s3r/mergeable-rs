use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::js;
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::sync::Arc;

pub mod assign;
pub mod checks;
pub mod close;
pub mod comment;
pub mod labels;
pub mod merge;
pub mod request_review;

#[async_trait]
pub trait Action: Send + Sync {
    fn name(&self) -> &'static str;
    fn supported_events(&self) -> &'static [&'static str];
    async fn before_validate(
        &self,
        context: &Context,
        rule: &Value,
        rule_name: Option<&Value>,
    ) -> JsResult<()>;
    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        rule_name: Option<&Value>,
        results: &Value,
    ) -> JsResult<()>;
}

pub fn create(key: &str) -> Option<Arc<dyn Action>> {
    let action: Arc<dyn Action> = match key {
        "assign" => Arc::new(assign::Assign),
        "checks" => Arc::new(checks::Checks::default()),
        "close" => Arc::new(close::Close),
        "comment" => Arc::new(comment::Comment),
        "labels" => Arc::new(labels::Labels),
        "merge" => Arc::new(merge::Merge),
        "request_review" => Arc::new(request_review::RequestReview),
        _ => return None,
    };
    Some(action)
}

pub fn is_event_supported(action: &dyn Action, event_name: &str) -> bool {
    event_aware::is_event_supported(action.supported_events(), event_name)
}

fn log_usage(action: &dyn Action, context: &Context, settings: &Value, log_type: &str) {
    tracing::info!(
        event_id = %context.event_id,
        repo = %context.repository_full_name(),
        action = action.name(),
        settings = %settings,
        "{log_type}"
    );
}

pub async fn process_before_validate(
    action: &dyn Action,
    context: &Context,
    rule: &Value,
    rule_name: Option<&Value>,
) -> JsResult<()> {
    log_usage(action, context, rule, "action_before_validate_execute");
    action.before_validate(context, rule, rule_name).await
}

pub async fn process_after_validate(
    action: &dyn Action,
    context: &Context,
    settings: &Value,
    rule_name: Option<&Value>,
    results: &Value,
) -> JsResult<()> {
    log_usage(action, context, settings, "action_after_validate_execute");
    action
        .after_validate(context, settings, rule_name, results)
        .await
}

pub fn get_actionables<'context>(
    context: &'context Context,
    results: &Value,
) -> Vec<Cow<'context, Value>> {
    let schedule_results = if context.event_name == "schedule" {
        js::get_path(results, &["validationSuites", "0", "schedule"])
            .filter(|schedule| js::is_truthy(schedule))
    } else {
        None
    };
    match schedule_results {
        Some(schedule) => {
            let issues = schedule
                .get("issues")
                .map(js::as_array_or_wrap)
                .unwrap_or_default();
            js::concat(&issues, schedule.get("pulls").unwrap_or(&Value::Null))
                .into_iter()
                .map(Cow::Owned)
                .collect()
        }
        None => {
            vec![event_aware::get_payload(context).map_or(Cow::Owned(Value::Null), Cow::Borrowed)]
        }
    }
}

pub fn is_comment_on_plain_issue(context: &Context) -> bool {
    context.event_name == "issue_comment"
        && !js::truthy(js::get_path(&context.payload, &["issue", "pull_request"]))
}
