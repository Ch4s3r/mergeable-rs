use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::filters::options::lib::{consolidate_results, construct_error};
use crate::flex;
use crate::js;
use crate::registry::Registry;
use serde_json::{Map, Value, json};

const OPTION_MISSING_ERROR_MESSAGE: &str = "Failed to filter because the 'filter' option is missing or empty. Please check the documentation.";

fn single_error(name: &str, settings: &Value, message: &str, filter_context: &Value) -> Value {
    consolidate_results(
        vec![construct_error(
            &json!(name),
            &json!(""),
            settings,
            message,
            None,
        )],
        filter_context,
    )
}

fn negated_status(status: Option<&str>) -> Option<&'static str> {
    match status {
        Some("error") => Some("error"),
        Some("pass") => Some("fail"),
        Some("fail") => Some("pass"),
        _ => None,
    }
}

fn prefix_descriptions(result: &Value, option_number: usize) -> JsResult<Vec<Value>> {
    let Some(Value::Array(sub_filters)) = result.get("filters") else {
        return Err(JsError::type_error("result.filters is not iterable"));
    };
    let result_name = js::to_js_string(result.get("name"));
    Ok(sub_filters
        .iter()
        .map(|sub_filter| {
            let mut described = sub_filter.clone();
            if let Value::Object(fields) = &mut described {
                let description = js::to_js_string(fields.get("description"));
                fields.insert(
                    "description".to_string(),
                    Value::String(format!(
                        "Option {option_number}: {result_name}: {description}"
                    )),
                );
            }
            described
        })
        .collect())
}

pub async fn process(
    context: &Context,
    settings: Option<&Value>,
    registry: &Registry,
    name: &str,
) -> JsResult<Value> {
    let filter_context = json!({"name": name});
    let listed_settings = settings.cloned().unwrap_or(Value::Null);

    if !matches!(settings, Some(Value::Array(items)) if !items.is_empty()) {
        return Ok(single_error(
            name,
            &listed_settings,
            OPTION_MISSING_ERROR_MESSAGE,
            &filter_context,
        ));
    }

    let rules = json!({"filter": listed_settings});

    if let Err(error) = registry.register_filters(&rules) {
        return Ok(single_error(
            name,
            &listed_settings,
            &format!("Unsupported filter {error}"),
            &filter_context,
        ));
    }

    let output = flex::filter_results(context, registry, &rules).await?;

    let mut filters = Vec::new();
    let mut status = if name == "And" { "pass" } else { "fail" };
    let mut count = 0;
    for result in &output {
        let result_status = result.get("status").and_then(Value::as_str);
        if result_status == Some("error") {
            status = "error";
        }
        if name == "Or" && result_status == Some("pass") && status != "error" {
            status = "pass";
        }
        if name == "And" && result_status == Some("fail") && status != "error" {
            status = "fail";
        }
        filters.extend(prefix_descriptions(result, count + 1)?);
        count += 1;
    }

    let mut final_status = Some(status);
    if name == "Not" {
        if count != 1 {
            return Ok(single_error(
                name,
                &listed_settings,
                &format!(
                    "Failed to validate because the 'filter' option does not have exactly one element, but {count}"
                ),
                &json!({}),
            ));
        }
        final_status = negated_status(
            output
                .first()
                .and_then(|only| only.get("status"))
                .and_then(Value::as_str),
        );
    }

    let mut combined = Map::new();
    if let Some(present_status) = final_status {
        combined.insert(
            "status".to_string(),
            Value::String(present_status.to_string()),
        );
    }
    combined.insert("name".to_string(), Value::String(name.to_string()));
    combined.insert("filters".to_string(), Value::Array(filters));
    Ok(Value::Object(combined))
}

#[cfg(test)]
mod tests {
    use crate::filters::and_operator::And;
    use crate::filters::lib::test_context::{PullRequest, pull_request_context};
    use crate::filters::not_operator::Not;
    use crate::filters::or_operator::Or;
    use crate::filters::{Filter, process_filter};
    use crate::registry::Registry;
    use serde_json::{Value, json};

    fn status_of(result: &Value) -> &str {
        result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn base_ref_must_include(regex: &str) -> Value {
        json!({"do": "baseRef", "must_include": {"regex": regex}})
    }

    async fn run(filter: &dyn Filter, settings: Value) -> Value {
        let context = pull_request_context(PullRequest {
            base_ref: "Topic 1",
            ..PullRequest::default()
        });
        process_filter(filter, &context, &settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn and_fails_when_its_only_sub_filter_fails() {
        let result = run(
            &And,
            json!({"do": "and", "filter": [base_ref_must_include("Topic 2")]}),
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn and_fails_when_the_first_of_two_sub_filters_fails() {
        let result = run(&And, json!({"do": "and", "filter": [base_ref_must_include("Topic 2"), base_ref_must_include("Topic 1")]})).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn and_passes_when_every_sub_filter_passes() {
        let settings = json!({"do": "and", "filter": [base_ref_must_include("Topic 1"), {"do": "baseRef", "must_exclude": {"regex": "Topic 2"}}]});
        let result = run(&And, settings).await;
        assert_eq!(status_of(&result), "pass");
        assert_eq!(result["name"], "And");
        assert_eq!(
            result["filters"][1]["description"],
            "Option 2: baseRef: baseRef must exclude 'Topic 2'"
        );
    }

    #[tokio::test]
    async fn missing_empty_or_non_list_filter_option_is_an_error() {
        for filter in [&And as &dyn Filter, &Or, &Not] {
            for settings in [
                json!({"do": "x"}),
                json!({"do": "x", "filter": ""}),
                json!({"do": "x", "filter": []}),
            ] {
                let result = run(filter, settings).await;
                assert_eq!(status_of(&result), "error");
                assert_eq!(
                    result["filters"][0]["description"],
                    "Failed to filter because the 'filter' option is missing or empty. Please check the documentation."
                );
            }
        }
    }

    #[tokio::test]
    async fn unknown_sub_filter_is_an_error() {
        for filter in [&And as &dyn Filter, &Or, &Not] {
            let result = run(filter, json!({"do": "x", "filter": [{"do": "missing"}]})).await;
            assert_eq!(status_of(&result), "error");
        }
    }

    #[tokio::test]
    async fn nested_or_inside_and_fails_when_the_sibling_fails() {
        let settings = json!({"do": "and", "filter": [
            {"do": "or", "filter": [base_ref_must_include("Topic 1"), base_ref_must_include("Topic 2")]},
            base_ref_must_include("Version 3"),
        ]});
        assert_eq!(status_of(&run(&And, settings).await), "fail");
    }

    #[tokio::test]
    async fn erroring_nested_sub_filter_makes_and_error() {
        let settings = json!({"do": "and", "filter": [
            {"do": "and", "filter": [{"do": "baseRef", "must_inclxude": {"regex": "Topic 1"}}, base_ref_must_include("Topic 2")]},
            base_ref_must_include("Version 3"),
        ]});
        assert_eq!(status_of(&run(&And, settings).await), "error");
    }

    #[tokio::test]
    async fn or_passes_when_any_sub_filter_passes() {
        let settings = json!({"do": "or", "filter": [base_ref_must_include("Topic 2"), base_ref_must_include("Topic 1")]});
        assert_eq!(status_of(&run(&Or, settings).await), "pass");
    }

    #[tokio::test]
    async fn or_fails_when_every_sub_filter_fails() {
        let settings = json!({"do": "or", "filter": [base_ref_must_include("Topic 2"), base_ref_must_include("Topic 3")]});
        assert_eq!(status_of(&run(&Or, settings).await), "fail");
    }

    #[tokio::test]
    async fn not_passes_when_its_sub_filter_fails() {
        let result = run(
            &Not,
            json!({"do": "not", "filter": [base_ref_must_include("Topic 2")]}),
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn not_fails_when_its_sub_filter_passes() {
        let result = run(
            &Not,
            json!({"do": "not", "filter": [base_ref_must_include("Topic 1")]}),
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn not_with_two_sub_filters_is_an_error_without_name() {
        let result = run(&Not, json!({"do": "not", "filter": [base_ref_must_include("Topic 1"), base_ref_must_include("Topic 2")]})).await;
        assert_eq!(status_of(&result), "error");
        assert_eq!(result.get("name"), None);
        assert_eq!(
            result["filters"][0]["description"],
            "Failed to validate because the 'filter' option does not have exactly one element, but 2"
        );
    }

    #[tokio::test]
    async fn erroring_nested_sub_filter_makes_not_error() {
        let settings = json!({"do": "not", "filter": [
            {"do": "not", "filter": [{"do": "baseRef", "must_inclxude": {"regex": "Topic 1"}}]},
        ]});
        assert_eq!(status_of(&run(&Not, settings).await), "error");
    }
}
