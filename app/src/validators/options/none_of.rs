use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::lib::OptionResult;
use serde_json::Value;

const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected string as input";
const LIST_NOT_FOUND_ERROR: &str = "Failed to run the test because 'none_of' option is not present. Please check README for more information about configuration";

pub fn process(_validator_context: &Value, input: &Value, rule: &Value) -> Result<OptionResult> {
    let filter = json_value::get(rule, "none_of");
    let Value::String(input_text) = input else {
        return Err(Error::general(UNKNOWN_INPUT_TYPE_ERROR));
    };
    let Some(Value::Array(list)) = filter else {
        return Err(Error::general(LIST_NOT_FOUND_ERROR));
    };
    let lowercase_input = input_text.to_lowercase();
    let is_listed = list
        .iter()
        .any(|item| item.as_str() == Some(lowercase_input.as_str()));
    let in_list = format!("'{input_text}' is in the none_of list'");
    let not_in_list = format!("'{input_text}' is not in the none_of list'");
    Ok(OptionResult::decided(!is_listed, not_in_list, in_list))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validators::options::lib::OptionStatus;
    use serde_json::json;

    #[test]
    fn lowercased_input_is_looked_up_in_the_list() {
        let rule = json!({"none_of": ["alice", "bob"]});
        let listed = process(&json!({"name": "author"}), &json!("Alice"), &rule).unwrap();
        assert_eq!(listed.status, OptionStatus::Fail);
        assert_eq!(listed.description, json!("'Alice' is in the none_of list'"));
        let unlisted = process(&json!({"name": "author"}), &json!("carol"), &rule).unwrap();
        assert_eq!(unlisted.status, OptionStatus::Pass);
        assert_eq!(
            unlisted.description,
            json!("'carol' is not in the none_of list'")
        );
    }

    #[test]
    fn non_string_input_and_missing_list_are_errors() {
        let input_error = process(&json!({}), &json!(["a"]), &json!({"none_of": []})).unwrap_err();
        assert_eq!(input_error.message(), UNKNOWN_INPUT_TYPE_ERROR);
        let list_error = process(&json!({}), &json!("a"), &json!({"none_of": "a"})).unwrap_err();
        assert_eq!(list_error.message(), LIST_NOT_FOUND_ERROR);
    }
}
