use crate::error::{Error, ErrorKind, Result};
use serde_json::Value;

const DATA_START: &str = "<!-- #mergeable-data";
const DATA_END: &str = "#mergeable-data -->";

#[must_use]
pub fn serialize(data: &Value) -> String {
    format!(
        "{DATA_START} {} {DATA_END}",
        crate::json_value::stringify(data)
    )
}

#[must_use]
pub fn exists(text: Option<&str>) -> bool {
    text.is_some_and(|content| content.contains(DATA_START) && content.contains(DATA_END))
}

pub fn deserialize(text: &str) -> Result<Value> {
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
        .map_err(|error| Error::new(ErrorKind::InvalidSyntax, error.to_string()))
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

    #[test]
    fn deserialize_reads_the_metadata_embedded_in_a_check_run_text() {
        let metadata = deserialize(
            r#"
    #### :x: Validator: TITLE * :x:
      ***title must begins with "feat,test,chore"
      *** Input : use-case: title Settings : ```{"begins_with":{"match":["feat","test","chore"]}}```
      <!-- #mergeable-data { "id": 1, "eventName": "pull_request", "action": "unlabeled" } #mergeable-data -->
  "#,
        )
        .unwrap();
        assert_eq!(metadata["id"], 1);
        assert_eq!(metadata["eventName"], "pull_request");
        assert_eq!(metadata["action"], "unlabeled");
    }

    #[test]
    fn serialize_wraps_the_metadata_in_an_html_comment_that_deserializes_back() {
        let metadata = json!({"id": 1, "eventName": "issues", "action": "milestoned"});
        let serialized = serialize(&metadata);
        assert_eq!(serialized, METADATA_TEXT);
        assert_eq!(deserialize(&serialized).unwrap(), metadata);
    }

    #[test]
    fn exists_requires_both_metadata_markers() {
        assert!(exists(Some(METADATA_TEXT)));
        assert!(!exists(Some("abc <!-- #mergeable-data")));
        assert!(!exists(Some("abc #mergeable-data -->")));
        assert!(!exists(None));
    }

    const METADATA_TEXT: &str = r#"<!-- #mergeable-data {"id":1,"eventName":"issues","action":"milestoned"} #mergeable-data -->"#;
}
