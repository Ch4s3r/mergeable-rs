use super::mock_context::{mock_context, mock_context_with, team_members_stub};
use mergeable::error::ErrorKind;
use mergeable::validators::options_processor::teams;
use serde_json::json;

#[tokio::test]
async fn teams_members_are_extracted_properly() {
    let members = ["member1", "member2"];
    let github = mock_context_with(
        &json!({}),
        vec![
            team_members_stub("org", "team1", &members),
            team_members_stub("org", "team2", &members),
        ],
    )
    .await;
    let team_members =
        teams::extract_team_members(&github.context, &[json!("org/team1"), json!("org/team2")])
            .await
            .expect("team members");
    assert_eq!(team_members, vec![json!("member1"), json!("member2")]);
}

#[tokio::test]
async fn throws_team_not_found_error_if_at_least_one_team_is_not_found() {
    let github = mock_context(&json!({})).await;
    let error =
        teams::extract_team_members(&github.context, &[json!("org/team1"), json!("org/team2")])
            .await
            .expect_err("missing team");
    assert_eq!(error.kind(), ErrorKind::TeamNotFound);
}
