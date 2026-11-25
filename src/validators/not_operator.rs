use crate::context::Context;
use crate::error::Result;
use crate::registry::Registry;
use crate::results::Connective;
use crate::validators::Validator;
use crate::validators::logical_connective;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Not;

#[async_trait]
impl Validator for Not {
    fn name(&self) -> &'static str {
        "not"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["*"]
    }

    fn supported_settings(&self) -> Value {
        json!({})
    }

    fn checks_settings(&self) -> bool {
        false
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        registry: &Registry,
    ) -> Result<Value> {
        logical_connective::process(context, settings.get("validate"), registry, Connective::Not)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::test_context::{PullRequest, pull_request_context};
    use crate::validators;

    async fn validate_with_milestone(milestone_title: Option<&str>, settings: &Value) -> Value {
        let mut context = pull_request_context(&PullRequest::default());
        context.payload.to_mut()["pull_request"]["milestone"] =
            milestone_title.map_or(Value::Null, |title| json!({"title": title}));
        validators::process_validate(&Not, &context, settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn passes_if_subtask_fails() {
        let settings = json!({"do": "not", "validate": [{"do": "milestone", "must_include": {"regex": "Version 2"}}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "pass");
    }

    #[tokio::test]
    async fn fails_if_subtask_passes() {
        let settings = json!({"do": "not", "validate": [{"do": "milestone", "must_include": {"regex": "Version 2"}}]});
        let validation = validate_with_milestone(Some("Version 2"), &settings).await;
        assert_eq!(validation["status"], "fail");
    }

    #[tokio::test]
    async fn error_when_validate_has_more_than_one_item() {
        let settings = json!({"do": "not", "validate": [{"do": "milestone", "must_include": {"regex": "Version 2"}}, {"do": "milestone", "must_include": {"regex": "Version 1"}}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_is_missing() {
        let validation = validate_with_milestone(Some("Version 1"), &json!({"do": "not"})).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_is_not_an_array() {
        let settings = json!({"do": "not", "validate": ""});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_is_empty() {
        let settings = json!({"do": "and", "validate": []});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_uses_unsupported_validators() {
        let settings = json!({"do": "and", "validate": [{"do": "missing"}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_if_a_sub_validator_errored() {
        let settings = json!({"do": "and", "validate": [
            {"do": "and", "validate": [
                {"do": "milestone", "must_inclxude": {"regex": "Version 1"}},
                {"do": "milestone", "must_include": {"regex": "Version 2"}}
            ]},
            {"do": "milestone", "must_include": {"regex": "Version 3"}}
        ]});
        let validation = validate_with_milestone(Some("Version 2"), &settings).await;
        assert_eq!(validation["status"], "error");
    }
}
