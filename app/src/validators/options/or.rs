use crate::error::Result;
use crate::validators::options::lib::OptionResult;
use crate::validators::options::lib::and_or_processor::{self, Connective};
use serde_json::Value;

pub async fn process(
    validator_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<OptionResult> {
    and_or_processor::process(validator_context, input, rule, Connective::Or).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validators::options::lib::OptionStatus;
    use serde_json::json;

    fn option_context() -> Value {
        json!({"name": "OptionName", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required"]})
    }

    #[tokio::test]
    async fn passes_when_one_sub_option_passes() {
        let rule =
            json!({"or": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(&option_context(), &json!(["A", "C"]), &rule)
                .await
                .unwrap()
                .status,
            OptionStatus::Pass
        );
    }

    #[tokio::test]
    async fn fails_when_no_sub_option_passes() {
        let rule = json!({"or": [{"required": {"reviewers": ["user1"]}}, {"required": {"reviewers": ["user2"]}}]});
        assert_eq!(
            process(
                &option_context(),
                &json!(["user0", "user5", "user3"]),
                &rule
            )
            .await
            .unwrap()
            .status,
            OptionStatus::Fail
        );
    }

    #[tokio::test]
    async fn missing_or_list_is_an_input_type_error() {
        let rule = json!({"and": {"must_include": {"regex": "A"}}});
        let error = process(&option_context(), &json!("the test"), &rule)
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
            process(&option_context(), &json!(["A"]), &wrong_sub_setting)
                .await
                .unwrap()
                .status,
            OptionStatus::Error
        );
        let unknown_option =
            json!({"or": [{"must_inclde": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(&option_context(), &json!(["A"]), &unknown_option)
                .await
                .unwrap()
                .status,
            OptionStatus::Error
        );
    }

    #[tokio::test]
    async fn nested_and_inside_or_is_evaluated() {
        let rule = json!({"or": [{"and": [{"must_include": {"regex": "A"}}, {"must_include": {"regex": "C"}}]}, {"must_include": {"regex": "Z"}}]});
        assert_eq!(
            process(&option_context(), &json!(["A", "C"]), &rule)
                .await
                .unwrap()
                .status,
            OptionStatus::Pass
        );
    }

    #[tokio::test]
    async fn empty_sub_option_is_a_type_error_on_status() {
        let rule = json!({"or": [{}]});
        let error = process(&option_context(), &json!("x"), &rule)
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot read properties of undefined (reading 'status')"
        );
    }
}
