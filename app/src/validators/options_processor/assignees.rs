use crate::error::JsResult;
use crate::js;
use crate::validators::options_processor::map_logins;
use serde_json::Value;

pub async fn process(payload: Option<&Value>) -> JsResult<Vec<Value>> {
    map_logins(js::read(payload, "assignees")?, "payload.assignees")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn assignee_logins_are_retrieved() {
        let pull_request =
            json!({"user": {"login": "creator"}, "number": 1, "assignees": [{"login": "bob"}]});
        assert_eq!(
            process(Some(&pull_request)).await.unwrap(),
            vec![json!("bob")]
        );
    }
}
