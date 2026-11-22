use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;
use tracing::Instrument;

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
        _context: &Context,
        _rule: &Value,
        _rule_name: Option<&Value>,
    ) -> Result<()> {
        Ok(())
    }
    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        rule_name: Option<&Value>,
        validation_report: &Value,
    ) -> Result<()>;
}

#[must_use]
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

pub fn spawn_detached(
    task_name: &'static str,
    task: impl Future<Output = Result<()>> + Send + 'static,
) {
    tokio::spawn(
        async move {
            if let Err(error) = task.await {
                tracing::error!(task = task_name, %error, "action.task.failed");
            }
        }
        .in_current_span(),
    );
}

pub async fn process_before_validate(
    action: &dyn Action,
    context: &Context,
    rule: &Value,
    rule_name: Option<&Value>,
) -> Result<()> {
    tracing::info!(
        action = action.name(),
        settings = %rule,
        "action.before_validate.started"
    );
    action.before_validate(context, rule, rule_name).await
}

pub async fn process_after_validate(
    action: &dyn Action,
    context: &Context,
    settings: &Value,
    rule_name: Option<&Value>,
    validation_report: &Value,
) -> Result<()> {
    tracing::info!(
        action = action.name(),
        %settings,
        "action.after_validate.started"
    );
    action
        .after_validate(context, settings, rule_name, validation_report)
        .await
}

pub fn actionables<'value>(
    context: &'value Context,
    validation_report: &'value Value,
) -> Vec<Cow<'value, Value>> {
    let schedule_results = if context.event_name == "schedule" {
        json_value::get_path(validation_report, &["validationSuites", "0", "schedule"])
            .filter(|schedule| json_value::is_truthy(schedule))
    } else {
        None
    };
    match schedule_results {
        Some(schedule) => {
            let issues = schedule
                .get("issues")
                .map_or(&[][..], json_value::as_array_or_wrap);
            json_value::concat(issues, schedule.get("pulls").unwrap_or(&json_value::NULL))
                .map(Cow::Borrowed)
                .collect()
        }
        None => {
            vec![event_aware::payload(context).map_or(Cow::Owned(Value::Null), Cow::Borrowed)]
        }
    }
}

#[must_use]
pub fn is_comment_on_plain_issue(context: &Context) -> bool {
    context.event_name == "issue_comment"
        && !json_value::truthy(json_value::get_path(
            &context.payload,
            &["issue", "pull_request"],
        ))
}
