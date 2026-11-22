use crate::context::Context;
use crate::error::{Error, Result};
use crate::filters::options::lib::{consolidate_results, construct_error};
use crate::json_value;
use crate::registry::Registry;
use crate::results::Status;
use crate::workflow;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connective {
    And,
    Or,
    Not,
}

impl Connective {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Connective::And => "And",
            Connective::Or => "Or",
            Connective::Not => "Not",
        }
    }

    fn combined_status(self, statuses: &[Option<Status>]) -> Option<Status> {
        let any = |wanted: Status| statuses.contains(&Some(wanted));
        match self {
            _ if any(Status::Error) && self != Connective::Not => Some(Status::Error),
            Connective::And => Some(if any(Status::Fail) {
                Status::Fail
            } else {
                Status::Pass
            }),
            Connective::Or => Some(if any(Status::Pass) {
                Status::Pass
            } else {
                Status::Fail
            }),
            Connective::Not => statuses.first().copied().flatten().map(Status::negated),
        }
    }
}

const OPTION_MISSING_ERROR_MESSAGE: &str = "Failed to filter because the 'filter' option is missing or empty. Please check the documentation.";

fn single_error(name: &str, settings: &Value, message: &str, filter_context: &Value) -> Value {
    consolidate_results(
        vec![construct_error(
            &json!(name),
            json!(""),
            settings,
            message,
            None,
        )],
        filter_context,
    )
}

fn prefix_descriptions(mut result: Value, option_number: usize) -> Result<Vec<Value>> {
    let result_name = json_value::to_display_string(result.get("name"));
    let Some(Value::Array(sub_filters)) = result.get_mut("filters").map(Value::take) else {
        return Err(Error::type_mismatch("result.filters is not iterable"));
    };
    Ok(sub_filters
        .into_iter()
        .map(|mut sub_filter| {
            if let Value::Object(fields) = &mut sub_filter {
                let description = json_value::to_display_string(fields.get("description"));
                fields.insert(
                    "description".to_string(),
                    Value::String(format!(
                        "Option {option_number}: {result_name}: {description}"
                    )),
                );
            }
            sub_filter
        })
        .collect())
}

pub async fn process(
    context: &Context,
    settings: Option<&Value>,
    registry: &Registry,
    connective: Connective,
) -> Result<Value> {
    let name = connective.name();
    let filter_context = json!({"name": name});
    let listed_settings = settings.unwrap_or(&Value::Null);

    if !matches!(settings, Some(Value::Array(items)) if !items.is_empty()) {
        return Ok(single_error(
            name,
            listed_settings,
            OPTION_MISSING_ERROR_MESSAGE,
            &filter_context,
        ));
    }

    let rules = json!({"filter": listed_settings});

    if let Err(error) = registry.register_filters(&rules) {
        return Ok(single_error(
            name,
            listed_settings,
            &format!("Unsupported filter {error}"),
            &filter_context,
        ));
    }

    let output = workflow::filter_results(context, registry, &rules).await?;
    let statuses: Vec<Option<Status>> = output.iter().map(Status::of).collect();
    let sub_filter_count = output.len();

    let mut filters = Vec::new();
    for (option_index, result) in output.into_iter().enumerate() {
        filters.extend(prefix_descriptions(result, option_index + 1)?);
    }

    if connective == Connective::Not && sub_filter_count != 1 {
        return Ok(single_error(
            name,
            listed_settings,
            &format!(
                "Failed to validate because the 'filter' option does not have exactly one element, but {sub_filter_count}"
            ),
            &json!({}),
        ));
    }

    let mut combined = Map::new();
    if let Some(status) = connective.combined_status(&statuses) {
        combined.insert("status".to_string(), status.into());
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
        let context = pull_request_context(&PullRequest {
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
