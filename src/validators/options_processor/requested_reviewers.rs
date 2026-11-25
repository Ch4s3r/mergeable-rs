use crate::error::Result;
use crate::json_value;
use crate::validators::options_processor::map_logins;
use serde_json::Value;

pub fn process(payload: Option<&Value>) -> Result<Vec<Value>> {
    map_logins(
        json_value::read(payload, "requested_reviewers")?,
        "payload.requested_reviewers",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requested_reviewer_logins_are_retrieved() {
        let pull_request = json!({"requested_reviewers": [{"login": "alice"}, {"login": "bob"}]});
        assert_eq!(
            process(Some(&pull_request)).unwrap(),
            vec![json!("alice"), json!("bob")]
        );
    }

    #[test]
    fn missing_list_is_a_type_error() {
        let error = process(Some(&json!({}))).unwrap_err();
        assert_eq!(
            error.message(),
            "Cannot read properties of undefined (reading 'map')"
        );
    }
}
