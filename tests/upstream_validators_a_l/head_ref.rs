use super::mock_context::mock_context;
use serde_json::{Value, json};

async fn validate_head_ref(head_ref: &str, settings: &Value) -> Value {
    mock_context(&json!({"headRef": head_ref}))
        .await
        .process_validate("headRef", settings)
        .await
}

#[tokio::test]
async fn fail_gracefully_if_invalid_regex() {
    let settings = json!({"do": "headRef", "must_exclude": {"regex": "@#$@#$@#$"}});
    assert_eq!(
        validate_head_ref("WIP HeadRef", &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn checks_that_it_fail_when_exclude_regex_is_in_head_ref() {
    let settings = json!({
        "do": "headRef",
        "must_include": {"regex": "^\\(feat\\)|^\\(doc\\)|^\\(fix\\)"},
        "must_exclude": {"regex": "wip"},
    });
    assert_eq!(
        validate_head_ref("WIP HeadRef", &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_head_ref("(feat) WIP HeadRef", &settings).await["status"],
        "fail"
    );
}

#[tokio::test]
async fn checks_that_advance_setting_of_must_include_works() {
    let test_message = "this is a test message";
    let settings = json!({
        "do": "headRef",
        "must_include": {"regex": "`^\\(feat\\)|^\\(doc\\)|^\\(fix\\)`", "message": test_message},
        "must_exclude": {"regex": "wip"},
    });
    let validation = validate_head_ref("include HeadRef", &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(validation["validations"][0]["description"], test_message);

    assert_eq!(
        validate_head_ref("(feat) WIP HeadRef", &settings).await["status"],
        "fail"
    );
}
