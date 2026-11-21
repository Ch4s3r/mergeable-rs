use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::js;
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

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"}
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let head_ref = js::read(js::read(event_aware::get_payload(context), "head")?, "ref")?
            .cloned()
            .unwrap_or(Value::Null);
        validators::process_options(settings, &head_ref, None).await
    }
}
