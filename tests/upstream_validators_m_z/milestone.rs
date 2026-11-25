use super::{MockedGithub, PullRequestFields, get_stub, pull_request_payload};
use mergeable::validators::milestone::Milestone;
use serde_json::{Value, json};

fn version_one_required() -> Value {
    json!({"do": "milestone", "must_include": {"regex": "Version 1"}})
}

async fn github_with_closed_issue_milestone(issue_milestone_title: &str) -> MockedGithub {
    MockedGithub::start(
        vec![get_stub(
            "/repos/owner/repo/issues/1",
            json!({"milestone": {"title": issue_milestone_title}}),
        )],
        pull_request_payload(PullRequestFields {
            body: Some("closes #1"),
            ..PullRequestFields::default()
        }),
    )
    .await
}

#[tokio::test]
async fn deep_validation_passes_when_the_pull_request_closes_an_issue_with_the_required_milestone()
{
    let github = github_with_closed_issue_milestone("Version 1").await;
    let validation = github
        .process_validate(&Milestone, &version_one_required())
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn deep_validation_fails_when_the_closed_issue_has_another_milestone() {
    let github = github_with_closed_issue_milestone("Version 2").await;
    let validation = github
        .process_validate(&Milestone, &version_one_required())
        .await;
    assert_eq!(validation["status"], "fail");
}
