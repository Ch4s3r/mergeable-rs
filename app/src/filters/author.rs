use crate::context::Context;
use crate::error::{Error, Result};
use crate::event_aware;
use crate::filters::{self, Filter};
use crate::json_value;
use crate::registry::Registry;
use crate::validators::options_processor::teams;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::borrow::Cow;

pub struct Author;

#[async_trait]
impl Filter for Author {
    fn name(&self) -> &'static str {
        "author"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": "string", "regex_flag": "string", "message": "string"},
            "team": "string",
            "one_of": "array",
            "none_of": "array",
        })
    }

    async fn filter(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let payload =
            event_aware::get_payload(context).ok_or_else(|| Error::missing_property("user"))?;
        let mut remaining_settings = Cow::Borrowed(settings);

        if json_value::truthy(settings.get("team")) {
            let result = teams::process_team_option(context, settings, Some(payload)).await?;
            if result.get("status").and_then(Value::as_str) != Some("pass") {
                return Ok(result);
            }
            if let Value::Object(fields) = remaining_settings.to_mut() {
                fields.shift_remove("team");
            }
        }

        let login = json_value::read(json_value::read(Some(payload), "user")?, "login")?
            .unwrap_or(&Value::Null);
        filters::process_options(self, context, login, &remaining_settings).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::lib::test_context::{PullRequest, pull_request_context};
    use crate::filters::process_filter;

    const AUTHOR_NAME: &str = "mergeabletestauthorname";
    const OTHER_AUTHOR_NAME: &str = "someone-else";

    fn status_of(result: &Value) -> &str {
        result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    async fn filter_author(settings: Value) -> Value {
        let context = pull_request_context(&PullRequest {
            author: AUTHOR_NAME,
            ..PullRequest::default()
        });
        process_filter(&Author, &context, &settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn unexpected_author_fails_must_include() {
        let result =
            filter_author(json!({"do": "author", "must_include": {"regex": OTHER_AUTHOR_NAME}}))
                .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn expected_author_passes_must_include() {
        let result =
            filter_author(json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}})).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn excluded_author_fails_must_exclude() {
        let result =
            filter_author(json!({"do": "author", "must_exclude": {"regex": AUTHOR_NAME}})).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn author_missing_from_one_of_list_fails() {
        let result = filter_author(json!({"do": "author", "one_of": [OTHER_AUTHOR_NAME]})).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn author_in_one_of_list_passes() {
        let result = filter_author(json!({"do": "author", "one_of": [AUTHOR_NAME]})).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn author_in_none_of_list_fails() {
        let result = filter_author(json!({"do": "author", "none_of": [AUTHOR_NAME]})).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn author_missing_from_none_of_list_passes() {
        let result = filter_author(json!({"do": "author", "none_of": [OTHER_AUTHOR_NAME]})).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn author_annotation_in_one_of_passes() {
        let result = filter_author(json!({"do": "author", "one_of": ["@author"]})).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn author_annotation_in_none_of_fails() {
        let result = filter_author(json!({"do": "author", "none_of": ["@author"]})).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn unsupported_setting_is_reported_as_error_with_the_setting_path() {
        let result = filter_author(json!({"do": "author", "must_include": {"regexp": "x"}})).await;
        assert_eq!(status_of(&result), "error");
        assert_eq!(
            result["filters"][0]["description"],
            "UnSupportedSettingError"
        );
        assert_eq!(
            result["filters"][0]["details"]["error"],
            "UnSupportedSettingError: filter/author: must_include.regexp option is not supported"
        );
    }
}
