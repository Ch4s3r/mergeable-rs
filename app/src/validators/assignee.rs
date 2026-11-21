use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::js;
use crate::registry::Registry;
use crate::validators::options_processor::map_logins;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Assignee;

#[async_trait]
impl Validator for Assignee {
    fn name(&self) -> &'static str {
        "assignee"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[
            "pull_request.*",
            "pull_request_review.*",
            "issues.*",
            "issue_comment.*",
        ]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "min": {"count": "number", "message": "string"},
            "max": {"count": "number", "message": "string"}
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let assignees = js::read(event_aware::get_payload(context), "assignees")?;
        let logins = map_logins(assignees, "assignees")?;
        validators::process_options(settings, &Value::Array(logins), None).await
    }
}
