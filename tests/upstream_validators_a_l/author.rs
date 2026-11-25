use super::mock_context::{
    MockedGithub, active_team_membership_stub, mock_context, mock_context_with, team_members_stub,
};
use serde_json::{Value, json};

const AUTHOR_NAME: &str = "mergeabletestauthorname";
const OTHER_AUTHOR_NAME: &str = "someone-else";

async fn authored_by(author: &str) -> MockedGithub {
    mock_context(&json!({"author": author})).await
}

async fn authored_by_member_of_team(author: &str, team_member: &str) -> MockedGithub {
    mock_context_with(
        &json!({"author": author}),
        vec![active_team_membership_stub("org", "team-slug", team_member)],
    )
    .await
}

async fn authored_by_author_with_team_members(author: &str, team_members: &[&str]) -> MockedGithub {
    mock_context_with(
        &json!({"author": author}),
        vec![team_members_stub("org", "team-slug", team_members)],
    )
    .await
}

async fn status_of(github: &MockedGithub, settings: &Value) -> Value {
    github.process_validate("author", settings).await["status"].clone()
}

#[tokio::test]
async fn should_fail_with_unexpected_author() {
    let settings = json!({"do": "author", "must_include": {"regex": OTHER_AUTHOR_NAME}});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn should_fail_with_unexpected_one_of_author() {
    let settings = json!({"do": "author", "one_of": [OTHER_AUTHOR_NAME]});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn should_pass_with_expected_author() {
    let settings = json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn should_pass_with_one_of_author() {
    let settings = json!({"do": "author", "one_of": [AUTHOR_NAME]});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn should_fail_with_excluded_author() {
    let settings = json!({"do": "author", "must_exclude": {"regex": AUTHOR_NAME}});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn should_fail_with_none_of_author() {
    let settings = json!({"do": "author", "none_of": [AUTHOR_NAME]});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn should_pass_with_excluded_author() {
    let settings = json!({"do": "author", "must_exclude": {"regex": OTHER_AUTHOR_NAME}});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn should_pass_with_none_of_author() {
    let settings = json!({"do": "author", "none_of": [OTHER_AUTHOR_NAME]});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn should_pass_with_expected_author_from_correct_team() {
    let settings =
        json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "team": "org/team-slug"});
    let github = authored_by_member_of_team(AUTHOR_NAME, AUTHOR_NAME).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn should_pass_with_one_of_author_from_correct_team() {
    let settings = json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "one_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members(AUTHOR_NAME, &[AUTHOR_NAME]).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn should_fail_with_expected_author_from_incorrect_team() {
    let settings =
        json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "team": "org/team-slug"});
    let github = authored_by(AUTHOR_NAME).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_fail_with_one_of_author_from_incorrect_team() {
    let settings = json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "one_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members(AUTHOR_NAME, &[]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_fail_with_unexpected_author_from_correct_team() {
    let settings = json!({"do": "author", "must_include": {"regex": OTHER_AUTHOR_NAME}, "team": "org/team-slug"});
    let github = authored_by_member_of_team(AUTHOR_NAME, AUTHOR_NAME).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_fail_with_one_of_author_from_correct_team() {
    let settings = json!({"do": "author", "must_include": {"regex": OTHER_AUTHOR_NAME}, "one_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members(AUTHOR_NAME, &[AUTHOR_NAME]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_pass_when_the_author_is_a_member_of_the_team() {
    let settings = json!({"do": "author", "team": "org/team-slug"});
    let github = authored_by_member_of_team(AUTHOR_NAME, AUTHOR_NAME).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn should_pass_when_the_author_is_one_of_the_members_of_the_team() {
    let settings = json!({"do": "author", "one_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members(AUTHOR_NAME, &[AUTHOR_NAME]).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn should_fail_when_the_author_is_not_a_member_of_the_team() {
    let settings = json!({"do": "author", "team": "org/team-slug"});
    let github = authored_by_member_of_team("mergeable", OTHER_AUTHOR_NAME).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_fail_when_the_author_is_not_one_of_the_members_of_the_team() {
    let settings = json!({"do": "author", "one_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members("mergeable", &[OTHER_AUTHOR_NAME]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_pass_when_the_author_is_not_member_of_the_none_of_team() {
    let settings = json!({"do": "author", "none_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members(AUTHOR_NAME, &[OTHER_AUTHOR_NAME]).await;
    assert_eq!(status_of(&github, &settings).await, "pass");
}

#[tokio::test]
async fn should_fail_when_the_author_is_member_of_the_none_of_team() {
    let settings = json!({"do": "author", "none_of": ["@org/team-slug"]});
    let github = authored_by_author_with_team_members(AUTHOR_NAME, &[AUTHOR_NAME]).await;
    assert_eq!(status_of(&github, &settings).await, "fail");
}

#[tokio::test]
async fn should_pass_when_the_author_is_one_of_at_author() {
    let settings = json!({"do": "author", "one_of": ["@author"]});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn should_fail_when_the_author_is_none_of_at_author() {
    let settings = json!({"do": "author", "none_of": ["@author"]});
    assert_eq!(
        status_of(&authored_by(AUTHOR_NAME).await, &settings).await,
        "fail"
    );
}
