use crate::error::Result;
use crate::json_value;
use crate::validators::options_processor::map_logins;
use serde_json::Value;

pub fn process(payload: Option<&Value>) -> Result<Vec<Value>> {
    map_logins(json_value::read(payload, "assignees")?, "payload.assignees")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assignee_logins_are_retrieved() {
        let pull_request =
            json!({"user": {"login": "creator"}, "number": 1, "assignees": [{"login": "bob"}]});
        assert_eq!(process(Some(&pull_request)).unwrap(), vec![json!("bob")]);
    }
}
