use crate::error::JsResult;
use crate::validators::options::lib::{OptionResult, and_or_processor};
use serde_json::Value;

pub async fn process(
    validator_context: &Value,
    input: &Value,
    rule: &Value,
) -> JsResult<OptionResult> {
    and_or_processor::process(validator_context, input, rule, "and").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn label_context() -> Value {
        json!({"name": "label", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required"]})
    }

    #[tokio::test]
    async fn passes_when_every_sub_option_passes() {
        let rule =
            json!({"and": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(&label_context(), &json!(["A", "C"]), &rule)
                .await
                .unwrap()
                .status,
            "pass"
        );
    }

    #[tokio::test]
    async fn fails_when_one_sub_option_fails() {
        let rule =
            json!({"and": [{"must_include": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        let result = process(&label_context(), &json!(["B"]), &rule)
            .await
            .unwrap();
        assert_eq!(result.status, "fail");
        assert_eq!(
            result.description,
            json!("(label does not include \"A\"  ***AND***  label does not exclude \"B\")")
        );
    }

    #[tokio::test]
    async fn non_list_setting_is_an_input_type_error() {
        let rule = json!({"and": {"must_include": {"regex": "A"}}});
        let error = process(&label_context(), &json!("the test"), &rule)
            .await
            .unwrap_err();
        assert_eq!(
            error.message,
            "Input type invalid, expected array type as input"
        );
    }

    #[tokio::test]
    async fn invalid_sub_option_settings_are_errors() {
        let wrong_sub_setting =
            json!({"and": [{"must_include": {"regexs": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(&label_context(), &json!(["B"]), &wrong_sub_setting)
                .await
                .unwrap()
                .status,
            "error"
        );
        let unknown_option =
            json!({"and": [{"must_inclde": {"regex": "A"}}, {"must_exclude": {"regex": "B"}}]});
        assert_eq!(
            process(&label_context(), &json!(["B"]), &unknown_option)
                .await
                .unwrap()
                .status,
            "error"
        );
    }

    #[tokio::test]
    async fn every_unsupported_sub_option_is_listed_in_the_error() {
        let rule =
            json!({"and": [{"must_inclde": {"regex": "A"}}, {"must_excalude": {"regex": "B"}}]});
        let result = process(&label_context(), &json!(["B"]), &rule)
            .await
            .unwrap();
        assert_eq!(result.status, "error");
        assert_eq!(
            result.description,
            json!(
                "Error occurred: \n- The 'must_inclde' option is not supported for 'label' validator, please see README for all available options \n- The 'must_excalude' option is not supported for 'label' validator, please see README for all available options \n"
            )
        );
    }
}
