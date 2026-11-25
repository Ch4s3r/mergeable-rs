use super::mock_context::{MockedGithub, active_team_membership_stub, mock_context_with};
use mergeable::validators::options_processor::owners;
use serde_json::{Value, json};

async fn owners_context(
    code_owners: &str,
    changed_files: &[&str],
    extra_stubs: Vec<Value>,
) -> MockedGithub {
    let compared_files: Vec<Value> = changed_files
        .iter()
        .map(|filename| json!({"filename": filename}))
        .collect();
    mock_context_with(
        &json!({"author": "creator", "number": 1, "codeowners": code_owners, "compareCommits": compared_files, "files": []}),
        extra_stubs,
    )
    .await
}

async fn owners_of(code_owners: &str, changed_files: &[&str]) -> Vec<Value> {
    owners_with_stubs(code_owners, changed_files, Vec::new()).await
}

async fn owners_with_stubs(
    code_owners: &str,
    changed_files: &[&str],
    extra_stubs: Vec<Value>,
) -> Vec<Value> {
    let github = owners_context(code_owners, changed_files, extra_stubs).await;
    let pull_request = github.context.payload["pull_request"].clone();
    owners::process(&pull_request, &github.context)
        .await
        .expect("owners")
}

#[tokio::test]
async fn that_star_works() {
    assert_eq!(
        owners_of("* @bob", &["first/second/third/dir/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("* @bob", &["another/file/path/test.js"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_star_dot_js_file_type_works() {
    assert_eq!(
        owners_of("*.js @bob", &["first/second/third/dir/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("*.go @bob", &["another/file/path/test.js"]).await,
        Vec::<Value>::new()
    );
    assert_eq!(
        owners_of("*.go @bob", &["another/file/path/test.go"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_slash_docs_slash_only_matches_docs_directory_in_root() {
    assert_eq!(
        owners_of(" /docs/ @bob", &["/docs/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of(" /docs/ @bob", &["another/file/docs/test.js"]).await,
        Vec::<Value>::new()
    );
}

#[tokio::test]
async fn that_slash_docs_slash_only_matches_all_files_in_all_subdirectory() {
    assert_eq!(
        owners_of("/docs/ @bob", &["/docs/long/path/to/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("/docs/ @bob", &["docs/test/anotherTest.js"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_slash_docs_slash_star_only_matches_all_files_in_that_directory() {
    assert_eq!(
        owners_of("/docs/* @bob", &["/docs/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("/docs/* @bob", &["/docs/not/matching/test.js"]).await,
        Vec::<Value>::new()
    );
}

#[tokio::test]
async fn that_double_star_slash_docs_matches_all_files_and_directories_in_docs() {
    assert_eq!(
        owners_of("**/docs @bob", &["first/second/docs/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("**/docs @bob", &["first/second/docs/inside/test.js"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_slash_docs_double_star_apps_matches_the_sub_directories_properly() {
    assert_eq!(
        owners_of("/docs/**/apps/ @bob", &["/docs/second/third/apps/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("/docs/**/apps/ @bob", &["/docs/third/apps/inside/test.js"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_slash_apps_double_star_matches_all_the_sub_directories_of_app() {
    assert_eq!(
        owners_of("/apps/** @bob", &["/apps/second/third/dir/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("/apps/** @bob", &["/apps/file/path/test.go"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_apps_slash_matches_all_instances_of_app() {
    assert_eq!(
        owners_of("apps/ @bob", &["first/second/third/apps/test.js"]).await,
        vec![json!("bob")]
    );
    assert_eq!(
        owners_of("apps/ @bob", &["another/file/path/apps/test.js"]).await,
        vec![json!("bob")]
    );
}

#[tokio::test]
async fn that_rules_that_comes_later_takes_higher_priority() {
    let code_owners = "*.js @bob \n/docs/ @hope";
    assert_eq!(
        owners_of(code_owners, &["/docs/test.js"]).await,
        vec![json!("hope")]
    );
    assert_eq!(
        owners_of(code_owners, &["another/file/path/test.js"]).await,
        vec![json!("bob")]
    );
}

fn test_team_memberships(members: &[&str]) -> Vec<Value> {
    members
        .iter()
        .map(|member| active_team_membership_stub("bob", "test-team", member))
        .collect()
}

#[tokio::test]
async fn teams_owners_are_processed_correctly() {
    let owners = owners_with_stubs(
        "*.js @bob/test-team @teamMember1 @member2",
        &["/docs/test.js"],
        test_team_memberships(&["teamMember1", "member2"]),
    )
    .await;
    assert_eq!(owners, vec![json!("teamMember1"), json!("member2")]);
}

#[tokio::test]
async fn non_team_member_cannot_approve_the_pr() {
    let owners = owners_with_stubs(
        "*.js @bob/test-team @teamMember1 @member2",
        &["/docs/test.js"],
        test_team_memberships(&["teamMember1", "member2"]),
    )
    .await;
    assert!(!owners.contains(&json!("member3")));
}

#[tokio::test]
async fn teams_owners_and_individuals_are_processed_correctly() {
    let owners = owners_with_stubs(
        "*.js @bob/test-team @teamMember1 @member2 @hope",
        &["/docs/test.js"],
        test_team_memberships(&["teamMember1", "member2"]),
    )
    .await;
    assert_eq!(
        owners,
        vec![json!("teamMember1"), json!("member2"), json!("hope")]
    );
}
