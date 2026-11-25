use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Title;

#[async_trait]
impl Validator for Title {
    fn name(&self) -> &'static str {
        "title"
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
            "ends_with": {"match": ["string", "array"], "message": "string"}
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let title =
            json_value::read(event_aware::payload(context), "title")?.unwrap_or(&Value::Null);
        validators::process_options(settings, title, None, &context.app.behavior.jira).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::test_context::{PullRequest, pull_request_context};

    const CONVENTIONAL_PREFIXES: &str = r"^\(feat\)|^\(doc\)|^\(fix\)";

    async fn validate_title(title: &str, settings: &Value) -> Value {
        let mut context = pull_request_context(&PullRequest::default());
        context.payload.to_mut()["pull_request"]["title"] = json!(title);
        validators::process_validate(&Title, &context, settings, &Registry::default())
            .await
            .unwrap()
    }

    fn prefix_required_and_wip_excluded() -> Value {
        json!({"do": "title", "must_include": {"regex": CONVENTIONAL_PREFIXES}, "must_exclude": {"regex": "wip"}})
    }

    fn begins_with(matching: &Value) -> Value {
        json!({"do": "title", "begins_with": {"match": matching}})
    }

    fn ends_with(matching: &Value) -> Value {
        json!({"do": "title", "ends_with": {"match": matching}})
    }

    #[tokio::test]
    async fn validate_returns_fail_for_wip_and_pass_for_conventional_prefix() {
        let settings = prefix_required_and_wip_excluded();
        assert_eq!(validate_title("wip", &settings).await["status"], "fail");
        assert_eq!(
            validate_title("(feat) something else", &settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn odd_exclude_regex_fails_gracefully_and_passes() {
        let settings = json!({"do": "title", "must_exclude": {"regex": "@#$@#$@#$"}});
        assert_eq!(
            validate_title("WIP Title", &settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn fails_when_exclude_regex_is_in_title() {
        let settings = prefix_required_and_wip_excluded();
        assert_eq!(
            validate_title("WIP Title", &settings).await["status"],
            "fail"
        );
        assert_eq!(
            validate_title("(feat) WIP Title", &settings).await["status"],
            "fail"
        );
    }

    #[tokio::test]
    async fn must_include_message_replaces_the_default_description() {
        let test_message = "this is a test message";
        let settings = json!({"do": "title", "must_include": {"regex": CONVENTIONAL_PREFIXES, "message": test_message}, "must_exclude": {"regex": "wip"}});
        let validation = validate_title("include Title", &settings).await;
        assert_eq!(validation["status"], "fail");
        assert_eq!(validation["validations"][0]["description"], test_message);
        assert_eq!(
            validate_title("(feat) WIP Title", &settings).await["status"],
            "fail"
        );
    }

    #[tokio::test]
    async fn begins_with_fails_when_title_does_not_begin_with_match() {
        let settings = begins_with(&json!("(test)"));
        let validation = validate_title("include Title", &settings).await;
        assert_eq!(validation["status"], "fail");
        assert_eq!(
            validation["validations"][0]["description"],
            "title must begins with \"(test)\""
        );
        assert_eq!(
            validate_title("(test) WIP Title", &settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn begins_with_accepts_any_match_of_an_array() {
        let settings = begins_with(&json!(["test1", "test2"]));
        let validation = validate_title("include Title", &settings).await;
        assert_eq!(validation["status"], "fail");
        assert_eq!(
            validation["validations"][0]["description"],
            "title must begins with \"test1,test2\""
        );
        assert_eq!(
            validate_title("test1 WIP Title", &settings).await["status"],
            "pass"
        );
        assert_eq!(
            validate_title("test2 WIP Title", &settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn ends_with_fails_when_title_does_not_end_with_match() {
        let settings = ends_with(&json!("(test)"));
        let validation = validate_title("include Title", &settings).await;
        assert_eq!(validation["status"], "fail");
        assert_eq!(
            validation["validations"][0]["description"],
            "title must end with \"(test)\""
        );
        assert_eq!(
            validate_title("WIP Title (test)", &settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn ends_with_accepts_any_match_of_an_array() {
        let settings = ends_with(&json!(["test", "test2"]));
        let validation = validate_title("include Title", &settings).await;
        assert_eq!(validation["status"], "fail");
        assert_eq!(
            validation["validations"][0]["description"],
            "title must end with \"test,test2\""
        );
        assert_eq!(
            validate_title("WIP Title test", &settings).await["status"],
            "pass"
        );
        assert_eq!(
            validate_title("WIP Title test2", &settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn fails_when_include_regex_is_missing_from_title() {
        let settings = prefix_required_and_wip_excluded();
        let validation = validate_title("include Title", &settings).await;
        assert_eq!(validation["status"], "fail");
        assert_eq!(
            validation["validations"][0]["description"],
            format!("title does not include \"{CONVENTIONAL_PREFIXES}\"")
        );
        assert_eq!(
            validate_title("(feat) WIP Title", &settings).await["status"],
            "fail"
        );
    }
}
