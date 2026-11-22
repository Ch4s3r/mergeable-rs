use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct HeadRef;

#[async_trait]
impl Validator for HeadRef {
    fn name(&self) -> &'static str {
        "headRef"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"}
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let head_ref = json_value::read(
            json_value::read(event_aware::payload(context), "head")?,
            "ref",
        )?
        .unwrap_or(&Value::Null);
        validators::process_options(settings, head_ref, None).await
    }
}
