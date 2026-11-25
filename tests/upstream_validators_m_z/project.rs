use super::{MockedGithub, PullRequestFields, get_stub, pull_request_payload};
use mergeable::validators::project::Project;
use serde_json::{Value, json};

fn project_one_required() -> Value {
    json!({"do": "project", "must_include": {"regex": "Project One"}})
}

async fn github_with_project_board(fields: PullRequestFields) -> MockedGithub {
    MockedGithub::start(
        vec![
            get_stub(
                "/repos/owner/repo/projects",
                json!([{"name": "Project One", "id": 1}, {"name": "Porject Two", "id": 2}]),
            ),
            get_stub("/projects/1/columns", json!([{"id": 1}])),
            get_stub("/projects/2/columns", json!([{"id": 1}])),
            get_stub(
                "/projects/columns/1/cards",
                json!([{"content_url": "testRepo/issues/1"}, {"content_url": "testRepo/issues/2"}]),
            ),
        ],
        pull_request_payload(fields),
    )
    .await
}

#[tokio::test]
async fn pull_request_on_the_project_board_is_mergeable() {
    let github = github_with_project_board(PullRequestFields {
        number: Some(1),
        ..PullRequestFields::default()
    })
    .await;
    let validation = github
        .process_validate(&Project, &project_one_required())
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn pull_request_missing_from_the_project_board_is_not_mergeable() {
    let github = github_with_project_board(PullRequestFields {
        number: Some(3),
        ..PullRequestFields::default()
    })
    .await;
    let validation = github
        .process_validate(&Project, &project_one_required())
        .await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn missing_project_description_names_the_required_project() {
    let github = github_with_project_board(PullRequestFields {
        number: Some(3),
        ..PullRequestFields::default()
    })
    .await;
    let validation = github
        .process_validate(&Project, &project_one_required())
        .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "Must be in the \"Project One\" project."
    );
}

#[tokio::test]
async fn deep_validation_passes_when_the_pull_request_closes_an_issue_on_the_board() {
    let github = github_with_project_board(PullRequestFields {
        number: Some(3),
        body: Some("closes #1"),
        ..PullRequestFields::default()
    })
    .await;
    let validation = github
        .process_validate(&Project, &project_one_required())
        .await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        validation["validations"][0]["description"],
        "Required Project is present"
    );
}
