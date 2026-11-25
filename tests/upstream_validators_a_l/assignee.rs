use super::mock_context::mock_context;
use serde_json::{Value, json};

async fn validate_with_assignee_count(assignee_count: usize, settings: &Value) -> Value {
    let assignees: Vec<Value> = (0..assignee_count)
        .map(|index| json!({"login": format!("user{index}")}))
        .collect();
    mock_context(&json!({"assignees": assignees}))
        .await
        .process_validate("assignee", settings)
        .await
}

#[tokio::test]
async fn that_mergeable_is_false_when_less_than_minimum() {
    let settings = json!({"do": "assignee", "min": {"count": 2}});
    let validation = validate_with_assignee_count(1, &settings).await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn that_mergeable_is_true_when_the_same_as_minimum() {
    let settings = json!({"do": "assignee", "min": {"count": 2}});
    let validation = validate_with_assignee_count(2, &settings).await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_mergeable_is_true_when_greater_than_minimum() {
    let settings = json!({"do": "assignee", "min": {"count": 2}});
    let validation = validate_with_assignee_count(3, &settings).await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_description_is_dynamic_based_on_minimum() {
    let settings = json!({"do": "assignee", "min": {"count": 5}});
    let validation = validate_with_assignee_count(1, &settings).await;
    assert_eq!(
        validation["validations"][0]["description"],
        "assignee count is less than \"5\""
    );
}

#[tokio::test]
async fn that_description_is_correct_when_mergeable() {
    let settings = json!({"do": "assignee", "min": {"count": 5}});
    let validation = validate_with_assignee_count(5, &settings).await;
    assert_eq!(
        validation["validations"][0]["description"],
        "assignee does have a minimum of '5'"
    );
}

#[tokio::test]
async fn checks_that_max_is_working() {
    let settings = json!({"do": "assignee", "max": {"count": 2}});
    let validation = validate_with_assignee_count(3, &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "assignee count is more than \"2\""
    );
    let validation = validate_with_assignee_count(2, &settings).await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn checks_that_advance_setting_message_is_working() {
    let settings = json!({"do": "assignee", "max": {"count": 2, "message": "test string"}});
    let validation = validate_with_assignee_count(3, &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(validation["validations"][0]["description"], "test string");
    let validation = validate_with_assignee_count(2, &settings).await;
    assert_eq!(validation["status"], "pass");
}
