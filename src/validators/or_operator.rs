use crate::context::Context;
use crate::error::Result;
use crate::registry::Registry;
use crate::results::Connective;
use crate::validators::Validator;
use crate::validators::logical_connective;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Or;

#[async_trait]
impl Validator for Or {
    fn name(&self) -> &'static str {
        "or"
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
        logical_connective::process(context, settings.get("validate"), registry, Connective::Or)
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
        validators::process_validate(&Or, &context, settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn runs_subtasks() {
        let settings = json!({"do": "or", "validate": [{"do": "milestone", "must_include": {"regex": "Version 2"}}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "fail");
    }

    #[tokio::test]
    async fn passes_with_second_task_when_first_fails() {
        let settings = json!({"do": "or", "validate": [{"do": "milestone", "must_include": {"regex": "Version 2"}}, {"do": "milestone", "must_include": {"regex": "Version 1"}}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "pass");
    }

    #[tokio::test]
    async fn passes_with_first_task_when_several_are_given() {
        let settings = json!({"do": "or", "validate": [{"do": "milestone", "must_include": {"regex": "Version 1"}}, {"do": "milestone", "must_include": {"regex": "Version 2"}}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "pass");
    }

    #[tokio::test]
    async fn error_when_validate_is_missing() {
        let validation = validate_with_milestone(Some("Version 1"), &json!({"do": "or"})).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_is_not_an_array() {
        let settings = json!({"do": "or", "validate": ""});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_is_empty() {
        let settings = json!({"do": "or", "validate": []});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn error_when_validate_uses_unsupported_validators() {
        let settings = json!({"do": "or", "validate": [{"do": "missing"}]});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "error");
    }

    #[tokio::test]
    async fn supports_nested_or_validator() {
        let settings = json!({"do": "or", "validate": [
            {"do": "or", "validate": [{"do": "milestone", "must_include": {"regex": "Version 1"}}, {"do": "milestone", "must_include": {"regex": "Version 2"}}]},
            {"do": "milestone", "must_include": {"regex": "Version 3"}}
        ]});
        let validation = validate_with_milestone(Some("Version 2"), &settings).await;
        assert_eq!(validation["status"], "pass");
    }

    #[tokio::test]
    async fn error_if_one_of_the_sub_validators_errored() {
        let settings = json!({"do": "or", "validate": [
            {"do": "or", "validate": [
                {"do": "milestone", "must_incdlude": {"regex": "Version 1"}},
                {"do": "milestone", "must_include": {"regex": "Version 2"}}
            ]},
            {"do": "milestone", "must_include": {"regex": "Version 3"}}
        ]});
        let validation = validate_with_milestone(Some("Version 2"), &settings).await;
        assert_eq!(validation["status"], "error");
    }
}
