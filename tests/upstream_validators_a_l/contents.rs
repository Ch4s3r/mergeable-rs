use super::mock_context::{MockedGithub, file_content_stub, mock_context, mock_context_with};
use serde_json::json;

async fn files_with_content(files: &[&str], file_content: &str) -> MockedGithub {
    let content_stubs = files
        .iter()
        .map(|file_path| file_content_stub(file_path, file_content))
        .collect();
    mock_context_with(&json!({"files": files}), content_stubs).await
}

#[tokio::test]
async fn files_pr_diff_option_works_correctly() {
    let settings =
        json!({"do": "contents", "files": {"pr_diff": true}, "must_exclude": {"regex": "test"}});

    let validation = files_with_content(&["package.json"], "testString")
        .await
        .process_validate("contents", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
    assert!(
        validation["validations"][0]["description"]
            .as_str()
            .is_some_and(|description| description.contains("package.json"))
    );

    let validation = files_with_content(&["package.json"], "string")
        .await
        .process_validate("contents", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn files_ignore_option_works_correctly() {
    let settings =
        json!({"do": "contents", "files": {"pr_diff": true}, "must_exclude": {"regex": "test"}});
    let github = files_with_content(&[".github/mergeable.yml", "package.json"], "testString").await;
    let validation = github.process_validate("contents", &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "Failed files : 'package.json'"
    );

    let ignoring_package_json = json!({
        "do": "contents",
        "files": {"pr_diff": true, "ignore": ["package.json"]},
        "must_exclude": {"regex": "test"},
    });
    let validation = files_with_content(&["package.json"], "string")
        .await
        .process_validate("contents", &ignoring_package_json)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn fail_gracefully_if_content_is_not_found() {
    let settings =
        json!({"do": "contents", "files": {"pr_diff": true}, "must_exclude": {"regex": "test"}});
    let github = mock_context(&json!({"files": ["package.json"]})).await;
    let validation = github.process_validate("contents", &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "Failed files : 'package.json (Not Found)'"
    );
}
