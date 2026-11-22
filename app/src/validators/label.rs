use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::registry::Registry;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Label;

#[async_trait]
impl Validator for Label {
    fn name(&self) -> &'static str {
        "label"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[
            "pull_request.*",
            "pull_request_review.*",
            "issues.*",
            "issue_comment.*",
        ]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "no_empty": {"enabled": "boolean", "message": "string"},
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"},
            "min": {"count": "number", "message": "string"},
            "max": {"count": "number", "message": "string"}
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let issue_number =
            json_value::read(event_aware::get_payload(context), "number")?.unwrap_or(&Value::Null);
        let labels = api::list_labels_on_issue(context, issue_number).await?;
        validators::process_options(settings, &Value::Array(labels), None).await
    }
}
