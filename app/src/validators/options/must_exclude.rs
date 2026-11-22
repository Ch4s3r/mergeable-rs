use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::output::{self, OptionResult};
use crate::validators::options::text_matching;
use crate::validators::options::user_pattern::UserPattern;
use serde_json::Value;

const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'must_exclude' option. Please check README for more information about configuration";
const UNKNOWN_INPUT_TYPE_ERROR: &str =
    "Input type invalid, expected either string or array of string as input";
const KEY_NOT_FOUND_ERROR: &str =
    "Input type is an object and requires providing a 'key' for the 'must_exclude' option.";

fn label_matches(regex: &UserPattern, label: &Value, key: Option<&Value>) -> Result<bool> {
    match label {
        Value::String(text) => Ok(regex.is_match(text)),
        _ if json_value::truthy(key) => {
            let property = json_value::read(Some(label), &json_value::to_display_string(key))?;
            Ok(regex.is_match(&json_value::to_display_string(property)))
        }
        _ => Err(Error::general(KEY_NOT_FOUND_ERROR)),
    }
}

fn every_label(
    regex: &UserPattern,
    labels: &[Value],
    key: Option<&Value>,
    expected: bool,
) -> Result<bool> {
    for label in labels {
        if label_matches(regex, label, key)? != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn some_label(regex: &UserPattern, labels: &[Value], key: Option<&Value>) -> Result<bool> {
    for label in labels {
        if label_matches(regex, label, key)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn excludes_pattern(
    regex: &UserPattern,
    input: &Value,
    key: Option<&Value>,
    all: bool,
) -> Result<bool> {
    match input {
        Value::String(text) => Ok(!regex.is_match(text)),
        Value::Array(labels) if all => every_label(regex, labels, key, false),
        Value::Array(labels) => Ok(!some_label(regex, labels, key)?),
        _ => Err(Error::general(UNKNOWN_INPUT_TYPE_ERROR)),
    }
}

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "must_exclude");
    let regex = json_value::read(filter, "regex")?;
    let key = json_value::read(filter, "key")?;
    let message = json_value::read(filter, "message")?;
    let Some(regex) = regex.filter(|present| json_value::is_truthy(present)) else {
        return Err(Error::general(REGEX_NOT_FOUND_ERROR));
    };
    let filter = filter.unwrap_or(&Value::Null);
    let regex_list = text_matching::concat_into_list(regex);
    let all = json_value::truthy(json_value::get(filter, "all"));
    let all_text = if all { "all" } else { "" };
    let joined_patterns = text_matching::join_list(regex_list, ", ");
    let validator_name = output::validator_name(validator_context);
    let success_description =
        format!("{validator_name} {all_text}must exclude '{joined_patterns}'");
    let failure_description = output::message_or_default(
        message,
        format!("{validator_name} {all_text}does not exclude \"{joined_patterns}\""),
    );
    let flags = text_matching::regex_flags(filter);

    let mut is_mergeable = true;
    for pattern in regex_list {
        let compiled = text_matching::compile_user_regex(pattern, &flags)?;
        if !excludes_pattern(&compiled, input, key, all)? {
            is_mergeable = false;
            break;
        }
    }
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

    fn status_for(rule: &Value, input: &Value) -> Status {
        process(
            &json!({"name": "label"}),
            input,
            &json!({"must_exclude": rule}),
        )
        .unwrap()
        .status
    }

    #[test]
    fn passes_when_no_label_matches_and_fails_otherwise() {
        assert_eq!(
            status_for(&json!({"regex": "test"}), &json!(["A", "B", "C"])),
            Status::Pass
        );
        assert_eq!(
            status_for(&json!({"regex": "test"}), &json!(["A", "B", "the test"])),
            Status::Fail
        );
    }

    #[test]
    fn missing_regex_is_an_error() {
        let error = process(
            &json!({"name": "label"}),
            &json!(["the test"]),
            &json!({"must_exclude": {"count": "test"}}),
        )
        .unwrap_err();
        assert_eq!(error.message(), REGEX_NOT_FOUND_ERROR);
    }

    #[test]
    fn all_sub_option_checks_every_label() {
        assert_eq!(
            status_for(
                &json!({"regex": "test", "all": true}),
                &json!(["A", "B", "the test"])
            ),
            Status::Fail
        );
        assert_eq!(
            status_for(
                &json!({"regex": "test", "all": true}),
                &json!(["A", "B", "the"])
            ),
            Status::Pass
        );
    }

    #[test]
    fn regex_flag_none_makes_matching_case_sensitive() {
        assert_eq!(
            status_for(
                &json!({"regex": "test", "regex_flag": "none"}),
                &json!(["A", "B", "Test"])
            ),
            Status::Pass
        );
    }

    #[test]
    fn every_regex_of_a_list_must_be_absent() {
        assert_eq!(
            status_for(&json!({"regex": ["test", "D"]}), &json!(["A", "B", "C"])),
            Status::Pass
        );
        assert_eq!(
            status_for(
                &json!({"regex": ["^the", "F", "G"]}),
                &json!(["A", "B", "the test"])
            ),
            Status::Fail
        );
    }

    #[test]
    fn title_with_odd_but_valid_regex_passes() {
        assert_eq!(
            status_for(&json!({"regex": "@#$@#$@#$"}), &json!("WIP Title")),
            Status::Pass
        );
    }
}
