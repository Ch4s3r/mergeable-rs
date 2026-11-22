use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::lib::{self, Affix, OptionResult};
use serde_json::Value;

const MATCH_NOT_FOUND_ERROR: &str = "Failed to run the test because 'match' is not provided for 'ends_with' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected string or Array as input";

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "ends_with");
    let match_value = json_value::read(filter, "match")?;
    let message = json_value::read(filter, "message")?;
    let Some(match_value) = match_value.filter(|present| json_value::is_truthy(present)) else {
        return Err(Error::general(MATCH_NOT_FOUND_ERROR));
    };
    let validator_name = lib::validator_name(validator_context);
    let match_text = json_value::to_display_string(Some(match_value));
    let success_description = format!("{validator_name} does end with '{match_text}'");
    let failure_description = lib::message_or_default(
        message,
        format!("{validator_name} must end with \"{match_text}\""),
    );
    let is_mergeable = lib::input_has_affix(input, match_value, Affix::Suffix)
        .map_err(|_| Error::general(UNKNOWN_INPUT_TYPE_ERROR))?;
    Ok(OptionResult::decided(
        is_mergeable,
        success_description,
        failure_description,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validators::options::lib::OptionStatus;
    use serde_json::json;

    fn status_for(match_value: &Value, input: &Value) -> OptionStatus {
        process(
            &json!({"name": "label"}),
            input,
            &json!({"ends_with": {"match": match_value}}),
        )
        .unwrap()
        .status
    }

    #[test]
    fn passes_when_input_ends_with_any_match_of_a_list() {
        let matches = json!(["end1", "end2"]);
        assert_eq!(
            status_for(&matches, &json!("the test end1")),
            OptionStatus::Pass
        );
        assert_eq!(
            status_for(&matches, &json!("the test end2")),
            OptionStatus::Pass
        );
        assert_eq!(
            status_for(&matches, &json!("some title")),
            OptionStatus::Fail
        );
    }

    #[test]
    fn passes_when_string_or_one_list_item_ends_with_match() {
        assert_eq!(
            status_for(&json!("test"), &json!("the test")),
            OptionStatus::Pass
        );
        assert_eq!(
            status_for(&json!("test"), &json!(["A", "B", "the test"])),
            OptionStatus::Pass
        );
        assert_eq!(
            status_for(&json!("test"), &json!(["the test"])),
            OptionStatus::Pass
        );
    }

    #[test]
    fn fails_when_input_does_not_end_with_match() {
        assert_eq!(
            status_for(&json!("test"), &json!("test the")),
            OptionStatus::Fail
        );
    }

    #[test]
    fn missing_match_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!("the test"),
            &json!({"ends_with": {"regex": "test"}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), MATCH_NOT_FOUND_ERROR);
    }

    #[test]
    fn non_string_input_is_an_input_type_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(3),
            &json!({"ends_with": {"match": "x"}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), UNKNOWN_INPUT_TYPE_ERROR);
    }
}
