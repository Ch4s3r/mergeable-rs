use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::js;
use crate::registry::Registry;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Description;

#[async_trait]
impl Validator for Description {
    fn name(&self) -> &'static str {
        "description"
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
            "no_empty": {"enabled": "boolean", "message": "string"},
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"}
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let body = js::read(event_aware::get_payload(context), "body")?;
        let empty_description = Value::String(String::new());
        let description = match body {
            Some(present) if js::is_truthy(present) => present,
            _ => &empty_description,
        };
        validators::process_options(settings, description, None).await
    }
}
