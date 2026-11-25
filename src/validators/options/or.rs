use crate::app_config::JiraSettings;
use crate::error::Result;
use crate::validators::options::and_or_processor::{self, Connective};
use crate::validators::options::output::OptionResult;
use serde_json::Value;

pub async fn process(
    validator_context: &Value,
    input: &Value,
    rule: &Value,
    jira: &JiraSettings,
) -> Result<OptionResult> {
    and_or_processor::process(validator_context, input, rule, Connective::Or, jira).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::Status;
    use serde_json::json;

    fn option_context() -> Value {
        json!({"name": "OptionName", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required"]})
    }

    #[tokio::test]
    async fn passes_when_one_sub_option_passes() {
        let rule =
            json!({"or": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(
                &option_context(),
                &json!(["A", "C"]),
                &rule,
                &crate::app_config::JiraSettings::default()
            )
            .await
            .unwrap()
            .status,
            Status::Pass
        );
    }

    #[tokio::test]
    async fn fails_when_no_sub_option_passes() {
        let rule = json!({"or": [{"required": {"reviewers": ["user1"]}}, {"required": {"reviewers": ["user2"]}}]});
        assert_eq!(
            process(
                &option_context(),
                &json!(["user0", "user5", "user3"]),
                &rule,
                &crate::app_config::JiraSettings::default()
            )
            .await
            .unwrap()
            .status,
            Status::Fail
        );
    }

    #[tokio::test]
    async fn missing_or_list_is_an_input_type_error() {
        let rule = json!({"and": {"must_include": {"regex": "A"}}});
        let error = process(
            &option_context(),
            &json!("the test"),
            &rule,
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected array type as input"
        );
    }

    #[tokio::test]
    async fn invalid_sub_options_are_errors() {
        let wrong_sub_setting =
            json!({"or": [{"must_include": {"regexs": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(
                &option_context(),
                &json!(["A"]),
                &wrong_sub_setting,
                &crate::app_config::JiraSettings::default()
            )
            .await
            .unwrap()
            .status,
            Status::Error
        );
        let unknown_option =
            json!({"or": [{"must_inclde": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(
                &option_context(),
                &json!(["A"]),
                &unknown_option,
                &crate::app_config::JiraSettings::default()
            )
            .await
            .unwrap()
            .status,
            Status::Error
        );
    }

    #[tokio::test]
    async fn nested_and_inside_or_is_evaluated() {
        let rule = json!({"or": [{"and": [{"must_include": {"regex": "A"}}, {"must_include": {"regex": "C"}}]}, {"must_include": {"regex": "Z"}}]});
        assert_eq!(
            process(
                &option_context(),
                &json!(["A", "C"]),
                &rule,
                &crate::app_config::JiraSettings::default()
            )
            .await
            .unwrap()
            .status,
            Status::Pass
        );
    }

    #[tokio::test]
    async fn empty_sub_option_is_a_type_error_on_status() {
        let rule = json!({"or": [{}]});
        let error = process(
            &option_context(),
            &json!("x"),
            &rule,
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot read properties of undefined (reading 'status')"
        );
    }

    async fn upstream_or(input: &Value, rule: &Value) -> Result<OptionResult> {
        process(
            &option_context(),
            input,
            rule,
            &crate::app_config::JiraSettings::default(),
        )
        .await
    }

    #[tokio::test]
    async fn return_pass_if_input_passes_one_of_the_or_conditions() {
        let rule =
            json!({"or": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_or(&json!(["A", "C"]), &rule).await.unwrap().status,
            Status::Pass
        );
    }

    #[tokio::test]
    async fn return_fail_if_none_of_the_input_passes_one_of_the_or_conditions() {
        let rule = json!({"or": [{"required": {"reviewers": ["user1"]}}, {"required": {"reviewers": ["user2"]}}]});
        let input = json!(["user0", "user5", "user3"]);
        assert_eq!(
            upstream_or(&input, &rule).await.unwrap().status,
            Status::Fail
        );
    }

    #[tokio::test]
    async fn return_error_if_inputs_are_not_in_expected_format() {
        let rule = json!({"and": {"must_include": {"regex": "A"}}});
        let error = upstream_or(&json!("the test"), &rule).await.unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected array type as input"
        );
    }

    #[tokio::test]
    async fn return_error_if_option_is_not_valid() {
        let rule =
            json!({"or": [{"must_include": {"regexs": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_or(&json!(["A"]), &rule).await.unwrap().status,
            Status::Error
        );
    }

    #[tokio::test]
    async fn return_error_if_sub_option_is_not_valid() {
        let rule =
            json!({"or": [{"must_inclde": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_or(&json!(["A"]), &rule).await.unwrap().status,
            Status::Error
        );
    }
}
