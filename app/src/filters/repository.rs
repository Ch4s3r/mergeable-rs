use crate::context::Context;
use crate::error::Result;
use crate::filters::Filter;
use crate::filters::options::lib::{consolidate_results, construct_output};
use crate::filters::options_processor::{name, topics, visibility};
use crate::json_value;
use crate::registry::Registry;
use async_trait::async_trait;
use serde_json::{Map, Value, json};

pub struct Repository;

fn single_setting(key: &str, settings: &Value) -> Value {
    let mut rule = Map::new();
    rule.insert(
        key.to_string(),
        settings.get(key).cloned().unwrap_or(Value::Null),
    );
    Value::Object(rule)
}

#[async_trait]
impl Filter for Repository {
    fn name(&self) -> &'static str {
        "repository"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "visibility": "string",
            "topics": {
                "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
                "must_exclude": {"regex": "string", "regex_flag": "string", "message": "string"},
            },
            "name": {
                "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
                "must_exclude": {"regex": "string", "regex_flag": "string", "message": "string"},
            },
        })
    }

    async fn filter(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let mut output = Vec::new();
        let filter_context = json!({"name": settings.get("do").unwrap_or(&Value::Null)});

        if json_value::truthy(settings.get("topics")) {
            let processed = topics::process(context, &filter_context, settings).await?;
            output.push(construct_output(
                &filter_context,
                processed.input,
                single_setting("topics", settings),
                processed.result,
                None,
            ));
        }

        if json_value::truthy(settings.get("visibility")) {
            let processed = visibility::process(context, settings)?;
            output.push(construct_output(
                &filter_context,
                processed.input,
                single_setting("visibility", settings),
                processed.result,
                None,
            ));
        }

        if json_value::truthy(settings.get("name")) {
            let processed = name::process(context, &filter_context, settings).await?;
            output.push(construct_output(
                &filter_context,
                processed.input,
                single_setting("name", settings),
                processed.result,
                None,
            ));
        }

        Ok(consolidate_results(output, &filter_context))
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

    async fn filter_repository(is_private: bool, repository_name: &str, settings: Value) -> Value {
        let context = pull_request_context(&PullRequest {
            repository_name,
            is_private,
            ..PullRequest::default()
        });
        process_filter(&Repository, &context, &settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn public_repository_fails_private_visibility() {
        let result = filter_repository(
            false,
            "repoName",
            json!({"do": "repository", "visibility": "private"}),
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn public_repository_passes_public_visibility() {
        let result = filter_repository(
            false,
            "repoName",
            json!({"do": "repository", "visibility": "public"}),
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn private_repository_fails_public_visibility() {
        let result = filter_repository(
            true,
            "repoName",
            json!({"do": "repository", "visibility": "public"}),
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn private_repository_passes_private_visibility() {
        let result = filter_repository(
            true,
            "repoName",
            json!({"do": "repository", "visibility": "private"}),
        )
        .await;
        assert_eq!(status_of(&result), "pass");
        assert_eq!(
            result["filters"][0]["details"],
            json!({"input": {"private": true}, "settings": {"visibility": "private"}})
        );
    }

    #[tokio::test]
    async fn repository_name_not_matching_must_include_fails() {
        let settings =
            json!({"do": "repository", "name": {"must_include": {"regex": "test-repo-2"}}});
        assert_eq!(
            status_of(&filter_repository(true, "test-repo", settings).await),
            "fail"
        );
    }

    #[tokio::test]
    async fn repository_name_matching_must_include_passes() {
        let settings =
            json!({"do": "repository", "name": {"must_include": {"regex": "test-repo-2"}}});
        assert_eq!(
            status_of(&filter_repository(true, "test-repo-2", settings).await),
            "pass"
        );
    }

    #[tokio::test]
    async fn repository_name_matching_must_exclude_fails() {
        let settings =
            json!({"do": "repository", "name": {"must_exclude": {"regex": "test-repo"}}});
        assert_eq!(
            status_of(&filter_repository(true, "test-repo", settings).await),
            "fail"
        );
    }

    #[tokio::test]
    async fn repository_name_not_matching_must_exclude_passes() {
        let settings =
            json!({"do": "repository", "name": {"must_exclude": {"regex": "test-repo"}}});
        assert_eq!(
            status_of(&filter_repository(true, "not-desired-repo-2", settings).await),
            "pass"
        );
    }
}
