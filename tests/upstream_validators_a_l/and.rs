use super::mock_context::{MockedGithub, mock_context};
use serde_json::{Value, json};

async fn milestone_context(milestone_title: &str) -> MockedGithub {
    mock_context(&json!({"milestone": {"title": milestone_title}})).await
}

async fn and_status(milestone_title: &str, settings: &Value) -> Value {
    let github = milestone_context(milestone_title).await;
    let validation = github.process_validate("and", settings).await;
    validation["status"].clone()
}

#[tokio::test]
async fn should_run_subtasks() {
    let settings = json!({"do": "and", "validate": [
        {"do": "milestone", "must_include": {"regex": "Version 2"}},
    ]});
    assert_eq!(and_status("Version 1", &settings).await, "fail");
}

#[tokio::test]
async fn should_return_output_of_second_task_if_first_fails() {
    let settings = json!({"do": "and", "validate": [
        {"do": "milestone", "must_include": {"regex": "Version 2"}},
        {"do": "milestone", "must_include": {"regex": "Version 1"}},
    ]});
    assert_eq!(and_status("Version 1", &settings).await, "fail");
}

#[tokio::test]
async fn should_return_output_of_first_task_to_pass_when_multiple_are_given() {
    let settings = json!({"do": "and", "validate": [
        {"do": "milestone", "must_include": {"regex": "Version 1"}},
        {"do": "milestone", "must_exclude": {"regex": "Version 2"}},
    ]});
    assert_eq!(and_status("Version 1", &settings).await, "pass");
}

#[tokio::test]
async fn error_is_returned_when_validate_is_missing() {
    let settings = json!({"do": "and"});
    assert_eq!(and_status("Version 1", &settings).await, "error");
}

#[tokio::test]
async fn error_is_returned_when_validate_is_not_an_array() {
    let settings = json!({"do": "and", "validate": ""});
    assert_eq!(and_status("Version 1", &settings).await, "error");
}

#[tokio::test]
async fn error_is_returned_when_validate_is_empty() {
    let settings = json!({"do": "and", "validate": []});
    assert_eq!(and_status("Version 1", &settings).await, "error");
}

#[tokio::test]
async fn error_is_returned_when_validate_uses_unsupported_classes() {
    let settings = json!({"do": "and", "validate": [{"do": "missing"}]});
    assert_eq!(and_status("Version 1", &settings).await, "error");
}

#[tokio::test]
async fn supports_nested_and_validator() {
    let settings = json!({"do": "and", "validate": [
        {"do": "or", "validate": [
            {"do": "milestone", "must_include": {"regex": "Version 1"}},
            {"do": "milestone", "must_include": {"regex": "Version 2"}},
        ]},
        {"do": "milestone", "must_include": {"regex": "Version 3"}},
    ]});
    assert_eq!(and_status("Version 2", &settings).await, "fail");
}

#[tokio::test]
async fn error_if_one_of_the_sub_validator_errored() {
    let settings = json!({"do": "and", "validate": [
        {"do": "and", "validate": [
            {"do": "milestone", "must_inclxude": {"regex": "Version 1"}},
            {"do": "milestone", "must_include": {"regex": "Version 2"}},
        ]},
        {"do": "milestone", "must_include": {"regex": "Version 3"}},
    ]});
    assert_eq!(and_status("Version 2", &settings).await, "error");
}
