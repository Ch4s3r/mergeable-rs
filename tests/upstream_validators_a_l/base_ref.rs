use super::mock_context::{MockedGithub, mock_context, mock_context_with, repository_stub};
use serde_json::{Value, json};

async fn pull_request_with_base_ref(base_ref: &str) -> MockedGithub {
    mock_context(&json!({"baseRef": base_ref})).await
}

async fn check_suite_with_base_refs(base_refs: &[&str]) -> MockedGithub {
    let mut github = mock_context(&json!({"eventName": "check_suite"})).await;
    let pull_requests: Vec<Value> = base_refs
        .iter()
        .map(|base_ref| json!({"base": {"ref": base_ref}}))
        .collect();
    github.payload_mut()["check_suite"]["pull_requests"] = Value::Array(pull_requests);
    github
}

async fn status_with_pulls(pulls: Value) -> MockedGithub {
    mock_context_with(
        &json!({"eventName": "status"}),
        vec![repository_stub("/commits/sha1/pulls", &pulls)],
    )
    .await
}

async fn status_with_open_pull_base_refs(base_refs: &[&str]) -> MockedGithub {
    let pulls: Vec<Value> = base_refs
        .iter()
        .map(|base_ref| json!({"state": "open", "base": {"ref": base_ref}}))
        .collect();
    status_with_pulls(Value::Array(pulls)).await
}

async fn status_of(github: &MockedGithub, settings: &Value) -> Value {
    github.process_validate("baseRef", settings).await["status"].clone()
}

#[tokio::test]
async fn validate_check_suite_called_for_check_suite_events() {
    let github = check_suite_with_base_refs(&["foo"]).await;
    let settings = json!({"do": "baseRef", "must_include": {"regex": "foo"}});
    let validation = github.process_validate("baseRef", &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        validation["validations"][0]["validations"][0]["details"]["input"],
        "foo"
    );
    assert_eq!(github.requested_paths(), Vec::<String>::new());
}

#[tokio::test]
async fn validate_status_called_for_status_events() {
    let github = status_with_open_pull_base_refs(&["foo"]).await;
    let settings = json!({"do": "baseRef", "must_include": {"regex": "foo"}});
    let validation = github.process_validate("baseRef", &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        validation["validations"][0]["validations"][0]["details"]["input"],
        "foo"
    );
    assert_eq!(
        github.requested_paths(),
        vec!["/repos/owner/repoName/commits/sha1/pulls"]
    );
}

#[tokio::test]
async fn process_options_called_for_non_check_suite_non_status_events() {
    let github = pull_request_with_base_ref("foo").await;
    let settings = json!({"do": "baseRef", "must_include": {"regex": "foo"}});
    let validation = github.process_validate("baseRef", &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(validation["validations"][0]["details"]["input"], "foo");
    assert_eq!(github.requested_paths(), Vec::<String>::new());
}

#[tokio::test]
async fn fail_gracefully_if_invalid_regex() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "@#$@#$@#$"}});
    assert_eq!(
        status_of(&pull_request_with_base_ref("WIP BaseRef").await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn checks_that_it_fail_when_exclude_regex_is_in_base_ref() {
    let settings = json!({
        "do": "baseRef",
        "must_include": {"regex": "^\\(feat\\)|^\\(doc\\)|^\\(fix\\)"},
        "must_exclude": {"regex": "wip"},
    });
    assert_eq!(
        status_of(&pull_request_with_base_ref("WIP BaseRef").await, &settings).await,
        "fail"
    );
    assert_eq!(
        status_of(
            &pull_request_with_base_ref("(feat) WIP BaseRef").await,
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn checks_that_it_passes_when_exclude_regex_is_not_in_base_ref() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    assert_eq!(
        status_of(&pull_request_with_base_ref("foo").await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn checks_that_it_passes_when_include_regex_is_in_base_ref() {
    let settings =
        json!({"do": "baseRef", "must_include": {"regex": "^\\(feat\\)|^\\(doc\\)|^\\(fix\\)"}});
    assert_eq!(
        status_of(&pull_request_with_base_ref("(feat) foo").await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn checks_that_advance_setting_of_must_include_works() {
    let test_message = "this is a test message";
    let settings = json!({
        "do": "baseRef",
        "must_include": {"regex": "^\\(feat\\)|^\\(doc\\)|^\\(fix\\)", "message": test_message},
        "must_exclude": {"regex": "wip"},
    });
    let validation = pull_request_with_base_ref("include BaseRef")
        .await
        .process_validate("baseRef", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(validation["validations"][0]["description"], test_message);

    assert_eq!(
        status_of(
            &pull_request_with_base_ref("(feat) WIP BaseRef").await,
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn fail_when_exclude_regex_is_in_base_ref_of_single_check_suite_pull_request() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    assert_eq!(
        status_of(&check_suite_with_base_refs(&["WIP foo"]).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn fail_when_exclude_regex_is_in_one_base_ref_of_multiple_check_suite_pull_requests() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    let github = check_suite_with_base_refs(&["foo", "WIP bar", "baz"]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn pass_when_exclude_regex_is_not_in_any_base_ref_of_multiple_check_suite_pull_requests() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    let github = check_suite_with_base_refs(&["foo", "bar", "baz"]).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn fail_when_include_regex_exists_and_there_are_no_pull_requests_in_check_suite() {
    let settings = json!({"do": "baseRef", "must_include": {"regex": "foo"}});
    assert_eq!(
        status_of(&check_suite_with_base_refs(&[]).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn fail_when_exclude_regex_is_in_base_ref_of_single_pull_request_related_to_status() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    assert_eq!(
        status_of(
            &status_with_open_pull_base_refs(&["WIP foo"]).await,
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn fail_when_exclude_regex_is_in_one_base_ref_of_multiple_pull_requests_related_to_status() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    let github = status_with_open_pull_base_refs(&["foo", "WIP bar", "baz"]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn pass_when_exclude_regex_is_not_in_any_base_ref_of_multiple_pull_requests_related_to_status()
 {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    let github = status_with_open_pull_base_refs(&["foo", "bar", "baz"]).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn fail_when_include_regex_exists_and_there_are_no_pull_requests_related_to_status() {
    let settings = json!({"do": "baseRef", "must_include": {"regex": "foo"}});
    assert_eq!(
        status_of(&status_with_open_pull_base_refs(&[]).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn pass_when_exclude_regex_is_only_in_base_ref_of_a_closed_pull_request_related_to_status() {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}});
    let github = status_with_pulls(json!([
        {"state": "open", "base": {"ref": "foo"}},
        {"state": "closed", "base": {"ref": "wip bar"}},
    ]))
    .await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn fail_with_media_type_when_exclude_regex_is_in_base_ref_of_single_pull_request_related_to_status()
 {
    let settings = json!({"do": "baseRef", "must_exclude": {"regex": "wip"}, "mediaType": {"previews": ["groot"]}});
    let github = status_with_open_pull_base_refs(&["WIP foo"]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
    assert_eq!(
        github.requested_paths(),
        vec!["/repos/owner/repoName/commits/sha1/pulls"]
    );
    assert_eq!(github.requested_queries(), vec![json!({})]);
}
