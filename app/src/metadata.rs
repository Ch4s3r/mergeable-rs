use crate::error::{JsError, JsResult};
use serde_json::Value;

const DATA_START: &str = "<!-- #mergeable-data";
const DATA_END: &str = "#mergeable-data -->";

#[must_use]
pub fn serialize(data: &Value) -> String {
    format!("{DATA_START} {} {DATA_END}", crate::js::stringify(data))
}

#[must_use]
pub fn exists(text: Option<&str>) -> bool {
    text.is_some_and(|content| content.contains(DATA_START) && content.contains(DATA_END))
}

pub fn deserialize(text: &str) -> JsResult<Value> {
    let begin = text
        .find(DATA_START)
        .map_or(DATA_START.len() - 1, |index| index + DATA_START.len());
    let end = text.find(DATA_END).unwrap_or(0);
    let embedded = if begin <= end {
        &text[begin..end]
    } else {
        &text[end..begin]
    };
    serde_json::from_str(embedded.trim())
        .map_err(|error| JsError::named("SyntaxError", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_round_trips_through_a_check_run_text() {
        let text = format!(
            "some text {}",
            serialize(&json!({"id": 3, "eventName": "pull_request", "action": "opened"}))
        );
        assert!(exists(Some(&text)));
        assert_eq!(
            deserialize(&text).unwrap(),
            json!({"id": 3, "eventName": "pull_request", "action": "opened"})
        );
    }
}
