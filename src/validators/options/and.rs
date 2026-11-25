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
    and_or_processor::process(validator_context, input, rule, Connective::And, jira).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::Status;
    use serde_json::json;

    fn label_context() -> Value {
        json!({"name": "label", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required"]})
    }

    #[tokio::test]
    async fn passes_when_every_sub_option_passes() {
        let rule =
            json!({"and": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(
                &label_context(),
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
    async fn fails_when_one_sub_option_fails() {
        let rule =
            json!({"and": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        let result = process(
            &label_context(),
            &json!(["B"]),
            &rule,
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Fail);
        assert_eq!(
            result.description,
            json!("(label does not include \"A\"  ***AND***  label does not exclude \"B\")")
        );
    }

    #[tokio::test]
    async fn non_list_setting_is_an_input_type_error() {
        let rule = json!({"and": {"must_include": {"regex": "A"}}});
        let error = process(
            &label_context(),
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
    async fn invalid_sub_option_settings_are_errors() {
        let wrong_sub_setting =
            json!({"and": [{"must_include": {"regexs": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(
                &label_context(),
                &json!(["B"]),
                &wrong_sub_setting,
                &crate::app_config::JiraSettings::default()
            )
            .await
            .unwrap()
            .status,
            Status::Error
        );
        let unknown_option =
            json!({"and": [{"must_inclde": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(
                &label_context(),
                &json!(["B"]),
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
    async fn every_unsupported_sub_option_is_listed_in_the_error() {
        let rule =
            json!({"and": [{"must_inclde": {"regex": "A"}}, {"must_excalude": {"regex": "B"}}]});
        let result = process(
            &label_context(),
            &json!(["B"]),
            &rule,
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Error);
        assert_eq!(
            result.description,
            json!(
                "Error occurred: \n- The 'must_inclde' option is not supported for 'label' validator, please see README for all available options \n- The 'must_excalude' option is not supported for 'label' validator, please see README for all available options \n"
            )
        );
    }

    async fn upstream_and(input: &Value, rule: &Value) -> Result<OptionResult> {
        process(
            &label_context(),
            input,
            rule,
            &crate::app_config::JiraSettings::default(),
        )
        .await
    }

    #[tokio::test]
    async fn return_pass_if_input_begins_with_the_rule() {
        let rule =
            json!({"and": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_and(&json!(["A", "C"]), &rule)
                .await
                .unwrap()
                .status,
            Status::Pass
        );
    }

    #[tokio::test]
    async fn return_fail_if_input_does_not_begins_with_the_rule() {
        let rule =
            json!({"and": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_and(&json!(["B"]), &rule).await.unwrap().status,
            Status::Fail
        );
    }

    #[tokio::test]
    async fn return_error_if_inputs_are_not_in_expected_format() {
        let rule = json!({"and": {"must_include": {"regex": "A"}}});
        let error = upstream_and(&json!("the test"), &rule).await.unwrap_err();
        assert_eq!(
            error.message(),
            "Input type invalid, expected array type as input"
        );
    }

    #[tokio::test]
    async fn return_error_if_option_is_not_valid() {
        let rule =
            json!({"and": [{"must_include": {"regexs": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_and(&json!(["B"]), &rule).await.unwrap().status,
            Status::Error
        );
    }

    #[tokio::test]
    async fn return_error_if_sub_option_is_not_valid() {
        let rule =
            json!({"and": [{"must_inclde": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            upstream_and(&json!(["B"]), &rule).await.unwrap().status,
            Status::Error
        );
    }

    #[tokio::test]
    async fn return_error_if_multiple_sub_option_is_not_valid() {
        let rule =
            json!({"and": [{"must_inclde": {"regex": "A"}}, {"must_excalude": {"regex": "B"}}]});
        let result = upstream_and(&json!(["B"]), &rule).await.unwrap();
        assert_eq!(result.status, Status::Error);
        assert_eq!(
            result.description,
            json!(
                "Error occurred: \n- The 'must_inclde' option is not supported for 'label' validator, please see README for all available options \n- The 'must_excalude' option is not supported for 'label' validator, please see README for all available options \n"
            )
        );
    }
}
