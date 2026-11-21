use crate::actions::comment::spawn_detached;
use crate::actions::{Action, get_actionables};
use crate::context::Context;
use crate::error::JsResult;
use crate::github::api;
use crate::js;
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

    async fn before_validate(
        &self,
        _context: &Context,
        _rule: &Value,
        _rule_name: Option<&Value>,
    ) -> JsResult<()> {
        Ok(())
    }

    async fn after_validate(
        &self,
        context: &Context,
        _settings: &Value,
        _rule_name: Option<&Value>,
        results: &Value,
    ) -> JsResult<()> {
        for issue in get_actionables(context, results) {
            let issue_number = js::read(Some(&issue), "number")?
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
