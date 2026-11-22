use crate::context::Context;
use crate::error::Result;
use crate::filters::options::lib::{consolidate_results, construct_error};
use crate::filters::{self, Filter};
use crate::json_value;
use crate::registry::Registry;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

const OPTION_NAMES: [&str; 5] = [
    "boolean",
    "must_include",
    "must_exclude",
    "one_of",
    "none_of",
];

pub struct Payload;

type FieldWalk<'walk> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'walk>>;

fn with_path_as_name(field: &Value, current_path: &str) -> Value {
    let mut named = field.clone();
    if let Value::Object(fields) = &mut named {
        fields.insert("do".to_string(), Value::String(current_path.to_string()));
    }
    named
}

fn recurse_through_fields<'walk>(
    filter: &'walk Payload,
    context: &'walk Context,
    current_path: String,
    output: &'walk mut Vec<Value>,
    payload: &'walk Value,
    field: &'walk Value,
) -> FieldWalk<'walk> {
    Box::pin(async move {
        for key in json_value::object_keys(Some(field))? {
            if key == "do" {
                continue;
            }
            if OPTION_NAMES.contains(&key.as_str()) {
                let named_field = with_path_as_name(field, &current_path);
                output
                    .push(filters::process_options(filter, context, payload, &named_field).await?);
                continue;
            }
            let nested_path = format!("{current_path}.{key}");
            match json_value::read(Some(payload), &key)? {
                None => output.push(construct_error(
                    &json!({"name": "payload"}),
                    json!(""),
                    field,
                    &format!("{nested_path} does NOT exist"),
                    None,
                )),
                Some(nested_payload) => {
                    let nested_field = json_value::get(field, &key).unwrap_or(&Value::Null);
                    recurse_through_fields(
                        filter,
                        context,
                        nested_path,
                        output,
                        nested_payload,
                        nested_field,
                    )
                    .await?;
                }
            }
        }
        Ok(())
    })
}

#[async_trait]
impl Filter for Payload {
    fn name(&self) -> &'static str {
        "payload"
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
        json!({})
    }

    fn checks_settings(&self) -> bool {
        false
    }

    async fn filter(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let mut output = Vec::new();
        recurse_through_fields(
            self,
            context,
            "payload".to_string(),
            &mut output,
            &context.payload,
            settings,
        )
        .await?;
        let filter_context = json!({"name": settings.get("do").unwrap_or(&Value::Null)});
        Ok(consolidate_results(output, &filter_context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::lib::test_context::context_for;
    use crate::filters::process_filter;

    fn status_of(result: &Value) -> &str {
        result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    async fn filter_payload(payload: Value, settings: Value) -> Value {
        let context = context_for("pull_request_review", payload);
        process_filter(&Payload, &context, &settings, &Registry::default())
            .await
            .unwrap()
    }

    fn review_payload(state: &str) -> Value {
        json!({"repository": {"full_name": "test-repo"}, "review": {"state": state, "user": {"login": "reviewer_1"}}})
    }

    fn labels_payload(label: &str) -> Value {
        json!({"repository": {"full_name": "test-repo"}, "pull_request": {"labels": [{"name": label}]}})
    }

    #[tokio::test]
    async fn checks_run_against_the_configured_field() {
        let settings = json!({"do": "payload", "review": {"state": {"must_include": {"regex": "changes_requested"}}}});
        assert_eq!(
            status_of(&filter_payload(review_payload("changes_requested"), settings.clone()).await),
            "pass"
        );
        assert_eq!(
            status_of(&filter_payload(review_payload("approved"), settings).await),
            "fail"
        );
    }

    #[tokio::test]
    async fn missing_field_is_reported_as_error() {
        let settings = json!({"do": "payload", "review": {"non_existent_field": {"must_include": {"regex": "yes"}}}});
        let result = filter_payload(review_payload("changes_requested"), settings).await;
        assert_eq!(status_of(&result), "error");
        assert_eq!(
            result["filters"][0]["description"],
            "payload.review.non_existent_field does NOT exist"
        );
    }

    #[tokio::test]
    async fn must_include_checks_objects_in_an_array_by_key() {
        let settings = json!({"do": "payload", "pull_request": {"labels": {"must_include": {"regex": "foo", "key": "name"}}}});
        assert_eq!(
            status_of(&filter_payload(labels_payload("foo"), settings.clone()).await),
            "pass"
        );
        assert_eq!(
            status_of(&filter_payload(labels_payload("bar"), settings).await),
            "fail"
        );
    }

    #[tokio::test]
    async fn must_exclude_checks_objects_in_an_array_by_key() {
        let settings = json!({"do": "payload", "pull_request": {"labels": {"must_exclude": {"regex": "foo", "key": "name"}}}});
        assert_eq!(
            status_of(&filter_payload(labels_payload("bar"), settings.clone()).await),
            "pass"
        );
        assert_eq!(
            status_of(&filter_payload(labels_payload("foo"), settings).await),
            "fail"
        );
    }

    #[tokio::test]
    async fn must_include_on_objects_without_key_is_an_error() {
        let settings = json!({"do": "payload", "pull_request": {"labels": {"must_include": {"regex": "foo"}}}});
        assert_eq!(
            status_of(&filter_payload(labels_payload("foo"), settings).await),
            "error"
        );
    }

    #[tokio::test]
    async fn must_exclude_on_objects_without_key_is_an_error() {
        let settings = json!({"do": "payload", "pull_request": {"labels": {"must_exclude": {"regex": "foo"}}}});
        assert_eq!(
            status_of(&filter_payload(labels_payload("bar"), settings).await),
            "error"
        );
    }

    #[tokio::test]
    async fn boolean_check_follows_the_draft_flag() {
        let settings =
            json!({"do": "payload", "pull_request": {"draft": {"boolean": {"match": true}}}});
        let draft = |is_draft: bool| json!({"repository": {"full_name": "test-repo"}, "pull_request": {"draft": is_draft}});
        assert_eq!(
            status_of(&filter_payload(draft(true), settings.clone()).await),
            "pass"
        );
        assert_eq!(
            status_of(&filter_payload(draft(false), settings).await),
            "fail"
        );
    }

    #[tokio::test]
    async fn option_result_is_named_after_the_payload_path() {
        let settings =
            json!({"do": "payload", "pull_request": {"draft": {"boolean": {"match": true}}}});
        let payload =
            json!({"repository": {"full_name": "test-repo"}, "pull_request": {"draft": false}});
        let result = filter_payload(payload, settings).await;
        assert_eq!(result["name"], "payload");
        assert_eq!(result["filters"][0]["name"], "payload.pull_request.draft");
        assert_eq!(
            result["filters"][0]["filters"][0]["description"],
            "The payload.pull_request.draft must be true"
        );
    }

    #[tokio::test]
    async fn nested_field_under_a_null_value_fails_with_a_type_error() {
        let settings = json!({"do": "payload", "pull_request": {"milestone": {"title": {"must_include": {"regex": "v1"}}}}});
        let payload =
            json!({"repository": {"full_name": "test-repo"}, "pull_request": {"milestone": null}});
        let context = context_for("pull_request", payload);
        let error = process_filter(&Payload, &context, &settings, &Registry::default())
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of null (reading 'title')"
        );
    }
}
