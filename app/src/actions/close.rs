use crate::actions::{Action, actionables, spawn_detached};
use crate::context::Context;
use crate::error::Result;
use crate::github::api;
use crate::json_value;
use async_trait::async_trait;
use serde_json::Value;

pub const SUPPORTED_EVENTS: [&str; 4] = [
    "pull_request.*",
    "issues.*",
    "issue_comment.*",
    "schedule.repository",
];

#[derive(Default)]
pub struct Close;

#[async_trait]
impl Action for Close {
    fn name(&self) -> &'static str {
        "close"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &SUPPORTED_EVENTS
    }

    async fn after_validate(
        &self,
        context: &Context,
        _settings: &Value,
        _rule_name: Option<&Value>,
        validation_report: &Value,
    ) -> Result<()> {
        for issue in actionables(context, validation_report) {
            let issue_number = json_value::read(Some(&*issue), "number")?
                .cloned()
                .unwrap_or(Value::Null);
            let task_context = context.clone();
            spawn_detached(context, "action/close", async move {
                api::update_issue_state(&task_context, &issue_number, "closed")
                    .await
                    .map(|_| ())
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_aware;

    #[test]
    fn close_supports_scheduled_runs_and_item_events() {
        for event in [
            "pull_request.opened",
            "issues.edited",
            "issue_comment.created",
            "schedule.repository",
        ] {
            assert!(
                event_aware::is_event_supported(&SUPPORTED_EVENTS, event),
                "{event}"
            );
        }
        assert!(!event_aware::is_event_supported(
            &SUPPORTED_EVENTS,
            "status.success"
        ));
    }
}
