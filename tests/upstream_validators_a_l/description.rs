use super::mock_context::mock_context;
use serde_json::{Map, Value, json};

async fn validate_body(body: Option<Value>, settings: &Value) -> Value {
    let mut options = Map::new();
    if let Some(description) = body {
        options.insert("body".to_owned(), description);
    }
    mock_context(&Value::Object(options))
        .await
        .process_validate("description", settings)
        .await
}

fn no_empty_settings() -> Value {
    json!({"do": "description", "no_empty": {"enabled": true}})
}

#[tokio::test]
async fn is_mergeable_is_true_if_the_pr_body_is_not_empty() {
    let validation = validate_body(
        Some(json!("This is a mock Description")),
        &no_empty_settings(),
    )
    .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn is_mergeable_is_false_if_the_pr_body_is_empty() {
    let settings = no_empty_settings();
    assert_eq!(
        validate_body(Some(json!("")), &settings).await["status"],
        "fail"
    );
    assert_eq!(validate_body(None, &settings).await["status"], "fail");
    assert_eq!(
        validate_body(Some(Value::Null), &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_body(Some(json!("Some Description")), &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn description_is_correct() {
    let settings = no_empty_settings();
    let validation = validate_body(Some(json!("")), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "The description can't be empty"
    );

    let validation = validate_body(Some(json!("Non empty Description")), &settings).await;
    assert_eq!(
        validation["validations"][0]["description"],
        "The description is not empty"
    );
}

#[tokio::test]
async fn must_include_works() {
    let settings = json!({"do": "description", "must_include": {"regex": "test string", "message": "failed test"}});
    assert_eq!(
        validate_body(Some(json!("test string included")), &settings).await["status"],
        "pass"
    );

    let validation = validate_body(Some(json!("Non empty Description")), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(validation["validations"][0]["description"], "failed test");
}

#[tokio::test]
async fn must_exclude_works() {
    let settings = json!({"do": "description", "must_exclude": {"regex": "test string", "message": "failed test"}});
    let validation = validate_body(Some(json!("test string included")), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(validation["validations"][0]["description"], "failed test");

    assert_eq!(
        validate_body(Some(json!("Non empty Description")), &settings).await["status"],
        "pass"
    );
}
