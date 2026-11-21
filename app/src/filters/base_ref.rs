use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::filters::{self, Filter};
use crate::js;
use crate::registry::Registry;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct BaseRef;

#[async_trait]
impl Filter for BaseRef {
    fn name(&self) -> &'static str {
        "baseRef"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": "string", "regex_flag": "string", "message": "string"},
        }))
    }

    async fn filter(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let payload = event_aware::get_payload(context);
        let base_ref = js::read(js::read(payload, "base")?, "ref")?
            .cloned()
            .unwrap_or(Value::Null);
        filters::process_options(self, context, &base_ref, settings).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::lib::test_context::{PullRequest, pull_request_context};
    use crate::filters::process_filter;

    fn status_of(result: &Value) -> &str {
        result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    async fn filter_base_ref(base_ref: &str, settings: Value) -> Value {
        let context = pull_request_context(PullRequest {
            base_ref,
            ..PullRequest::default()
        });
        process_filter(&BaseRef, &context, &settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn base_ref_not_matching_must_include_fails() {
        let result = filter_base_ref(
            "some-ref",
            json!({"do": "baseRef", "must_include": {"regex": "some-other-ref"}}),
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn base_ref_matching_must_include_passes() {
        let result = filter_base_ref(
            "some-ref",
            json!({"do": "baseRef", "must_include": {"regex": "some-ref"}}),
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn base_ref_matching_must_exclude_fails() {
        let result = filter_base_ref(
            "some-ref",
            json!({"do": "baseRef", "must_exclude": {"regex": "some-ref"}}),
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn base_ref_not_matching_must_exclude_passes() {
        let result = filter_base_ref(
            "some-ref",
            json!({"do": "baseRef", "must_exclude": {"regex": "some-other-ref"}}),
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }
}
