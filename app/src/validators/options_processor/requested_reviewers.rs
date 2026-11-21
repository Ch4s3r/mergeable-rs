use crate::error::JsResult;
use crate::js;
use crate::validators::options_processor::map_logins;
use serde_json::Value;

pub async fn process(payload: Option<&Value>) -> JsResult<Vec<Value>> {
    map_logins(
        js::read(payload, "requested_reviewers")?,
        "payload.requested_reviewers",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn requested_reviewer_logins_are_retrieved() {
        let pull_request = json!({"requested_reviewers": [{"login": "alice"}, {"login": "bob"}]});
        assert_eq!(
            process(Some(&pull_request)).await.unwrap(),
            vec![json!("alice"), json!("bob")]
        );
    }

    #[tokio::test]
    async fn missing_list_is_a_type_error() {
        let error = process(Some(&json!({}))).await.unwrap_err();
        assert_eq!(
            error.message,
            "Cannot read properties of undefined (reading 'map')"
        );
    }
}
