use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::output::{self, OptionResult};
use crate::validators::options::text_matching::{self, Affix};
use serde_json::Value;

const MATCH_NOT_FOUND_ERROR: &str = "Failed to run the test because 'match' is not provided for 'begins_with' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected string or Array as input";

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "begins_with");
    let match_value = json_value::read(filter, "match")?;
    let message = json_value::read(filter, "message")?;
    let Some(match_value) = match_value.filter(|present| json_value::is_truthy(present)) else {
        return Err(Error::general(MATCH_NOT_FOUND_ERROR));
    };
    let validator_name = output::validator_name(validator_context);
    let match_text = json_value::to_display_string(Some(match_value));
    let success_description = format!("{validator_name} does begins with '{match_text}'");
    let failure_description = output::message_or_default(
        message,
        format!("{validator_name} must begins with \"{match_text}\""),
    );
    let is_mergeable = text_matching::input_has_affix(input, match_value, Affix::Prefix)
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
    use crate::results::Status;
    use serde_json::json;

    fn status_for(match_value: &Value, input: &Value) -> Status {
        process(
            &json!({"name": "label"}),
            input,
            &json!({"begins_with": {"match": match_value}}),
        )
        .unwrap()
        .status
    }

    #[test]
    fn passes_when_input_begins_with_any_match_of_a_list() {
        let matches = json!(["feat", "core"]);
        assert_eq!(status_for(&matches, &json!("feat: the test")), Status::Pass);
        assert_eq!(status_for(&matches, &json!("core: the test")), Status::Pass);
        assert_eq!(status_for(&matches, &json!("some title")), Status::Fail);
    }

    #[test]
    fn passes_when_string_or_one_list_item_begins_with_match() {
        assert_eq!(status_for(&json!("the"), &json!("the test")), Status::Pass);
        assert_eq!(
            status_for(&json!("the"), &json!(["A", "B", "the test"])),
            Status::Pass
        );
    }

    #[test]
    fn fails_when_input_does_not_begin_with_match() {
        assert_eq!(status_for(&json!("the"), &json!("test the")), Status::Fail);
    }

    #[test]
    fn missing_match_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!("the test"),
            &json!({"begins_with": {"regex": "the"}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), MATCH_NOT_FOUND_ERROR);
    }

    #[test]
    fn failure_description_lists_the_match() {
        let result = process(
            &json!({"name": "title"}),
            &json!("include Title"),
            &json!({"begins_with": {"match": ["test1", "test2"]}}),
        )
        .unwrap();
        assert_eq!(
            result.description,
            json!("title must begins with \"test1,test2\"")
        );
    }
}
