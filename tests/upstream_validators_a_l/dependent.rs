use super::mock_context::mock_context;
use serde_json::{Value, json};

async fn validate_files(files: Value, settings: &Value) -> Value {
    mock_context(&json!({"files": files}))
        .await
        .process_validate("dependent", settings)
        .await
}

#[tokio::test]
async fn dependent_files_with_modified_changed_file_exists_and_files_found() {
    let settings = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json", "yarn.lock"]}});
    let validation = validate_files(
        json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
        &settings,
    )
    .await;
    assert_eq!(validation["status"], "pass");

    let one_dependent_file = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json"]}});
    let validation = validate_files(
        json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
        &one_dependent_file,
    )
    .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn dependent_files_with_modified_changed_file_exists_and_files_not_found() {
    let settings = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json"]}});
    let validation = validate_files(json!(["package.json", "a.js", "b.js"]), &settings).await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn dependent_files_with_modified_modified_does_not_exist_and_files_not_found() {
    let settings = json!({"do": "dependent", "changed": {"file": "package.json", "files": ["package-lock.json"]}});
    let validation = validate_files(json!(["package.json", "a.js", "b.js"]), &settings).await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn dependent_files_with_modified_required_sub_option_works_as_alias_for_files() {
    let settings = json!({"do": "dependent", "changed": {"file": "package.json", "required": ["package-lock.json"]}});
    let validation = validate_files(json!([]), &settings).await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn dependent_files_with_modified_error_when_file_sub_option_is_missing() {
    let settings = json!({"do": "dependent", "changed": {"required": ["package-lock.json"]}});
    let validation = validate_files(json!([]), &settings).await;
    assert_eq!(validation["status"], "error");
    assert_eq!(
        validation["validations"][0]["description"],
        "Failed to validate because the 'file' sub option for 'changed' option is missing. Please check the documentation"
    );
}

#[tokio::test]
async fn dependent_files_with_modified_glob_works_with_changed_file_option() {
    let settings =
        json!({"do": "dependent", "changed": {"file": "**/*.js", "files": ["package-lock.json"]}});
    assert_eq!(
        validate_files(json!(["a.js"]), &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_files(json!(["test/test.js"]), &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_files(json!(["test/test.js", "package-lock.json"]), &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn that_mergeable_is_true_if_none_of_the_dependent_file_is_modified() {
    let settings = json!({"do": "dependent", "files": ["a.js", "b.go"]});
    assert_eq!(validate_files(json!([]), &settings).await["status"], "pass");
}

#[tokio::test]
async fn that_mergeable_is_true_if_all_of_the_dependent_file_is_modified() {
    let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"]});
    assert_eq!(
        validate_files(json!(["package.json", "yarn.lock", "a.js"]), &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn that_mergeable_is_true_if_one_of_the_dependent_file_is_added() {
    let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"]});
    let files = json!([
        {"filename": "package.json", "status": "added"},
        {"filename": "yarn.lock", "status": "modified"},
    ]);
    assert_eq!(validate_files(files, &settings).await["status"], "pass");
}

#[tokio::test]
async fn that_mergeable_is_false_when_only_one_of_the_dependent_file_is_added() {
    let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"]});
    let files = json!([{"filename": "package.json", "status": "added"}]);
    assert_eq!(validate_files(files, &settings).await["status"], "fail");
}

#[tokio::test]
async fn that_mergeable_is_false_when_only_some_of_the_dependent_files_are_modified() {
    let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"]});
    assert_eq!(
        validate_files(json!(["package.json", "a.js", "b.js"]), &settings).await["status"],
        "fail"
    );
}

#[tokio::test]
async fn test_description_is_correct() {
    let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"]});
    let validation = validate_files(json!(["package.json"]), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "One or more files (yarn.lock) are missing from your pull request because they are dependent on the following: package.json"
    );
}

#[tokio::test]
async fn test_that_custom_message_is_correct() {
    let settings = json!({"do": "dependent", "files": ["package.json", "yarn.lock"], "message": "Test Message"});
    let validation = validate_files(json!(["package.json"]), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(validation["validations"][0]["description"], "Test Message");
}
