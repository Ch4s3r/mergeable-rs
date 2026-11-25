use super::mock_context::{
    MockedGithub, active_team_membership_stub, mock_context, mock_context_with,
};
use serde_json::{Value, json};

fn now_milliseconds() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn approvals_by_numbered_users(approval_count: usize) -> Vec<Value> {
    let now = now_milliseconds();
    (0..approval_count)
        .map(|index| {
            json!({
                "user": {"login": format!("user{index}")},
                "state": "APPROVED",
                "submitted_at": now + i64::try_from(index).expect("small index"),
            })
        })
        .collect()
}

fn review(login: &str, state: &str, submitted_at: i64) -> Value {
    json!({"user": {"login": login}, "state": state, "submitted_at": submitted_at})
}

fn approvals_without_time(logins: &[&str]) -> Vec<Value> {
    logins
        .iter()
        .map(|login| json!({"user": {"login": login}, "state": "APPROVED"}))
        .collect()
}

fn logins(users: &[&str]) -> Vec<Value> {
    users.iter().map(|login| json!({"login": login})).collect()
}

async fn reviewed(reviews: Vec<Value>) -> MockedGithub {
    mock_context(&json!({"reviews": reviews, "files": []})).await
}

async fn reviewed_with(
    options: &Value,
    reviews: Vec<Value>,
    extra_stubs: Vec<Value>,
) -> MockedGithub {
    let mut options = options.clone();
    options["reviews"] = Value::Array(reviews);
    options["files"] = json!([]);
    mock_context_with(&options, extra_stubs).await
}

fn code_owner_options(code_owner: &str) -> Value {
    json!({"codeowners": format!("* @{code_owner}"), "compareCommits": [{"filename": "a.js"}]})
}

fn first_description(validation: &Value) -> &Value {
    &validation["validations"][0]["description"]
}

#[tokio::test]
async fn that_approvals_work_with_no_owners_file() {
    let settings = json!({"do": "approval", "min": {"count": 2}});
    let validation = reviewed(approvals_by_numbered_users(1))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn that_mergeable_is_false_when_less_than_minimum() {
    let settings = json!({"do": "approval", "min": {"count": 2}});
    let validation = reviewed(approvals_by_numbered_users(1))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn that_mergeable_is_true_when_the_same_as_minimum() {
    let settings = json!({"do": "approval", "min": {"count": 2}});
    let validation = reviewed(approvals_by_numbered_users(2))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_mergeable_is_true_when_greater_than_minimum() {
    let settings = json!({"do": "approval", "min": {"count": 2}});
    let validation = reviewed(approvals_by_numbered_users(3))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_description_is_dynamic_based_on_minimum() {
    let settings = json!({"do": "approval", "min": {"count": 5}});
    let validation = reviewed(approvals_by_numbered_users(3))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        first_description(&validation),
        "approval count is less than \"5\""
    );
}

#[tokio::test]
async fn that_description_is_null_when_mergeable() {
    let settings = json!({"do": "approval", "min": {"count": 5}});
    let validation = reviewed(approvals_by_numbered_users(5))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        first_description(&validation),
        "approval does have a minimum of '5'"
    );
}

#[tokio::test]
async fn mergeable_is_false_if_required_members_have_not_approved() {
    let reviews = vec![
        json!({"user": {"login": "userA"}, "state": "APPROVED"}),
        json!({"user": {"login": "userB"}, "state": "APPROVED"}),
    ];
    let settings = json!({"do": "approval", "required": {"reviewers": ["userA", "userC"]}});
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(first_description(&validation), "approval: userC required");
}

#[tokio::test]
async fn mergeable_is_false_if_excluded_members_have_approved() {
    let reviews = vec![json!({"user": {"login": "userA"}, "state": "APPROVED"})];
    let settings = json!({"do": "approval", "min": {"count": 1}, "exclude": {"users": ["userA"]}});
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        first_description(&validation),
        "approval count is less than \"1\""
    );
}

#[tokio::test]
async fn mergeable_passes_if_required_user_has_approved() {
    let reviews = vec![
        json!({"user": {"login": "userA"}, "state": "APPROVED"}),
        json!({"user": {"login": "userB"}, "state": "NOT_APPROVED"}),
        json!({"user": {"login": "userC"}, "state": "APPROVED"}),
    ];
    let settings = json!({"do": "approval", "required": {"reviewers": ["userA", "userC"]}});
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn validate_correctly_when_one_required_user_approved_but_followed_up_with_comments() {
    let now = now_milliseconds();
    let reviews = vec![
        review("userA", "COMMENTED", now - 5000),
        review("userB", "COMMENTED", now - 4000),
        review("userA", "APPROVED", now - 3000),
        review("userB", "COMMENTED", now - 2000),
        review("userB", "APPROVED", now - 1000),
        review("userA", "COMMENTED", now),
        review("userC", "COMMENTED", now),
    ];
    let settings = json!({"do": "approval", "required": {"reviewers": ["userA"]}});
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        validation["validations"][0]["details"]["input"],
        json!(["userB", "userA"])
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn validate_correctly_when_one_required_user_approved_but_followed_up_with_request_change_or_pending()
 {
    let now = now_milliseconds();
    let reviews = vec![
        review("userA", "COMMENTED", now - 5000),
        review("userB", "COMMENTED", now - 4000),
        review("userA", "APPROVED", now - 3000),
        review("userB", "COMMENTED", now - 2000),
        review("userB", "APPROVED", now - 1000),
        review("userA", "CHANGES_REQUESTED", now - 500),
        review("userC", "APPROVED", now),
    ];
    let github = reviewed(reviews).await;
    let validation = github
        .process_validate(
            "approvals",
            &json!({"do": "approval", "required": {"reviewers": ["userA", "userB"]}}),
        )
        .await;
    assert_eq!(
        validation["validations"][0]["details"]["input"],
        json!(["userC", "userB"])
    );
    assert_eq!(validation["status"], "fail");

    let validation = github
        .process_validate(
            "approvals",
            &json!({"do": "approval", "required": {"reviewers": ["userA"]}}),
        )
        .await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn validate_correctly_with_reviews_more_than_30() {
    let now = now_milliseconds();
    let mut reviews = vec![review("user1", "APPROVED", now - 5000)];
    reviews.extend((0..40).map(|index| review(&format!("user{index}"), "COMMENTED", now + index)));
    reviews.push(review("user2", "APPROVED", now + 1000));
    let settings = json!({"do": "approval", "required": {"reviewers": ["user1", "user2"]}});
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        validation["validations"][0]["details"]["input"],
        json!(["user2", "user1"])
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn pr_creator_is_removed_from_required_reviewer_list() {
    let settings = json!({"do": "approval", "required": {"reviewers": ["creator", "userC"]}});
    let validation = reviewed(Vec::new())
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(first_description(&validation), "approval: userC required");
}

#[tokio::test]
async fn checks_that_latest_review_is_used() {
    let now = now_milliseconds();
    let reviews = vec![
        review("userA", "APPROVED", now),
        review("userA", "CHANGES_REQUESTED", now + 1000),
    ];
    let settings = json!({"do": "approval", "required": {"reviewers": ["userA"]}});
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(first_description(&validation), "approval: userA required");
}

#[tokio::test]
async fn mergeable_advance_setting_min_works() {
    let settings =
        json!({"do": "approval", "min": {"count": 2, "message": "This is a test message"}});
    let validation = reviewed(approvals_by_numbered_users(1))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(first_description(&validation), "This is a test message");

    let validation = reviewed(approvals_by_numbered_users(3))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn mergeable_advance_setting_max_works() {
    let settings =
        json!({"do": "approval", "max": {"count": 2, "message": "This is a test message"}});
    let validation = reviewed(approvals_by_numbered_users(3))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(first_description(&validation), "This is a test message");

    let validation = reviewed(approvals_by_numbered_users(1))
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn required_owners_that_owners_enabled_will_check_owner_file_and_append_it_to_required_list()
{
    let settings = json!({"do": "approval", "required": {"reviewers": ["john"], "owners": true}});

    let owned_by_bob = reviewed_with(&code_owner_options("bob"), Vec::new(), Vec::new()).await;
    let validation = owned_by_bob.process_validate("approvals", &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        first_description(&validation),
        "approval: john ,bob(Code Owner) required"
    );

    let without_owners = reviewed(Vec::new()).await;
    let validation = without_owners
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(first_description(&validation), "approval: john required");
}

#[tokio::test]
async fn required_owners_that_owners_enabled_will_check_owner_file_and_is_mergeable_when_approval_is_given()
 {
    let settings = json!({"do": "approval", "required": {"owners": true}});

    let not_yet_approved = reviewed_with(&code_owner_options("bob"), Vec::new(), Vec::new()).await;
    let validation = not_yet_approved
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        first_description(&validation),
        "approval: bob(Code Owner) required"
    );

    let approved_by_bob = reviewed_with(
        &code_owner_options("bob"),
        approvals_without_time(&["bob"]),
        Vec::new(),
    )
    .await;
    let validation = approved_by_bob
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn required_owners_that_owners_disabled_will_not_call_owners_process() {
    let github = reviewed_with(&code_owner_options("bob"), Vec::new(), Vec::new()).await;
    github
        .process_validate("approvals", &json!({"do": "approval"}))
        .await;
    let requested_paths = github.requested_paths();
    assert!(
        !requested_paths
            .iter()
            .any(|path| path.contains("CODEOWNERS") || path.contains("/compare/"))
    );
}

#[tokio::test]
async fn that_assignees_are_not_required_when_assignees_is_not_specified() {
    let settings = json!({"do": "approval", "required": {"reviewers": ["bob"]}});
    let github = reviewed_with(
        &json!({"assignees": logins(&["john"])}),
        approvals_without_time(&["bob"]),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_assignees_are_not_required_when_assignees_is_false() {
    let settings =
        json!({"do": "approval", "required": {"assignees": false, "reviewers": ["bob"]}});
    let github = reviewed_with(
        &json!({"assignees": logins(&["john"])}),
        approvals_without_time(&["bob"]),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_assignees_are_required_when_assignees_is_true() {
    let settings = json!({"do": "approval", "required": {"assignees": true}});
    let assigned_to_john = json!({"assignees": logins(&["john"])});

    let github = reviewed_with(
        &assigned_to_john,
        approvals_without_time(&["bob"]),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: john(Assignee) required"
    );
    assert_eq!(validation["status"], "fail");

    let github = reviewed_with(
        &assigned_to_john,
        approvals_without_time(&["bob", "john"]),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_assignees_appends_to_the_reviewers_list_correctly() {
    let settings = json!({"do": "approval", "required": {"assignees": true, "reviewers": ["bob"]}});
    let assigned_to_john = json!({"assignees": logins(&["john"])});

    let github = reviewed_with(&assigned_to_john, Vec::new(), Vec::new()).await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: bob ,john(Assignee) required"
    );
    assert_eq!(validation["status"], "fail");

    let github = reviewed_with(
        &assigned_to_john,
        approvals_without_time(&["bob", "john"]),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn that_requested_reviewers_appends_to_the_reviewers_list_correctly() {
    let settings =
        json!({"do": "approval", "required": {"requested_reviewers": true, "reviewers": ["bob"]}});
    let review_requested_from_john =
        json!({"assignees": [], "requestedReviewers": logins(&["john"])});

    let github = reviewed_with(&review_requested_from_john, Vec::new(), Vec::new()).await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: bob ,john(Requested Reviewer) required"
    );
    assert_eq!(validation["status"], "fail");

    let github = reviewed_with(
        &review_requested_from_john,
        approvals_without_time(&["bob", "john"]),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
    assert_eq!(validation["status"], "pass");
}

fn approval_followed_by_requested_changes() -> Vec<Value> {
    let now = now_milliseconds();
    vec![
        review("userA", "APPROVED", now),
        review("userB", "CHANGES_REQUESTED", now + 1000),
    ]
}

fn approvals_by_user_a_and_user_b() -> Vec<Value> {
    let now = now_milliseconds();
    vec![
        review("userA", "APPROVED", now),
        review("userB", "APPROVED", now + 1000),
    ]
}

#[tokio::test]
async fn blocks_if_changes_are_requested() {
    let settings = json!({"do": "approval", "block": {"changes_requested": true}});
    let validation = reviewed(approval_followed_by_requested_changes())
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(
        first_description(&validation),
        "Please resolve all the changes requested"
    );
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn blocks_if_changes_are_requested_while_other_options_are_also_passed_in() {
    let settings = json!({"do": "approval", "block": {"changes_requested": true}, "required": {"reviewers": ["userA"]}});
    let validation = reviewed(approval_followed_by_requested_changes())
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        validation["validations"][1]["description"],
        "Please resolve all the changes requested"
    );
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn returns_proper_error_when_team_provided_is_not_found() {
    let settings = json!({"do": "approval", "limit": {"teams": ["test-org/test-team"]}, "required": {"reviewers": ["userA"]}});
    let validation = reviewed(approval_followed_by_requested_changes())
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(2));
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        validation["validations"][1]["description"],
        "approval: all required reviewers have approved"
    );
}

#[tokio::test]
async fn required_owners_limit_teams_option_works_properly() {
    let settings = json!({"do": "approval", "limit": {"teams": ["test-org/test-team"]}, "required": {"reviewers": ["userA"]}});
    let github = reviewed_with(
        &json!({}),
        approvals_by_user_a_and_user_b(),
        vec![active_team_membership_stub(
            "test-org",
            "test-team",
            "userB",
        )],
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(2));
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][1]["description"],
        "approval: userA required"
    );
}

#[tokio::test]
async fn required_owners_limit_users_option_works_properly() {
    let github = reviewed(approvals_by_user_a_and_user_b()).await;

    let settings = json!({"do": "approval", "limit": {"users": ["userB"]}, "min": {"count": 2}});
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(2));
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][1]["description"],
        "approval count is less than \"2\""
    );

    let settings =
        json!({"do": "approval", "limit": {"users": ["userB", "userA"]}, "min": {"count": 2}});
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(2));
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn required_owners_limit_owners_option_works_properly() {
    let settings =
        json!({"do": "approval", "limit": {"owners": true}, "required": {"reviewers": ["userA"]}});
    let github = reviewed_with(
        &code_owner_options("userB"),
        approvals_by_user_a_and_user_b(),
        Vec::new(),
    )
    .await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(2));
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][1]["description"],
        "approval: userA required"
    );
}

#[tokio::test]
async fn required_owners_limit_option_will_create_an_info_validation() {
    let settings = json!({"do": "approval", "limit": {"owners": true}});
    let reviews = vec![review("userA", "APPROVED", now_milliseconds())];
    let github = reviewed_with(&code_owner_options("userB"), reviews, Vec::new()).await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(validation["validations"].as_array().map(Vec::len), Some(1));
    assert_eq!(validation["status"], "pass");
    assert_eq!(validation["validations"][0]["status"], "info");
    assert_eq!(
        first_description(&validation),
        "Only approvals from following sources are counted"
    );
}

#[tokio::test]
async fn required_owners_limit_owners_passes_when_codeowners_file_is_missing() {
    let settings = json!({"do": "approval", "limit": {"owners": true}, "min": {"count": 1}});
    let reviews = vec![review("userA", "APPROVED", now_milliseconds())];
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        first_description(&validation),
        "Only approvals from following sources are counted"
    );
    assert_eq!(
        validation["validations"][1]["description"],
        "approval does have a minimum of '1'"
    );
}

#[tokio::test]
async fn required_owners_limit_owners_passes_when_codeowner_approved_and_teams_are_empty() {
    let settings = json!({"do": "approval", "limit": {"owners": true}, "min": {"count": 1}});
    let reviews = vec![review("userA", "APPROVED", now_milliseconds())];
    let github = reviewed_with(&code_owner_options("userA"), reviews, Vec::new()).await;
    let validation = github.process_validate("approvals", &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        first_description(&validation),
        "Only approvals from following sources are counted"
    );
    assert_eq!(
        validation["validations"][1]["description"],
        "approval does have a minimum of '1'"
    );
}

#[tokio::test]
async fn required_owners_required_owners_passes_when_codeowners_file_is_missing_and_other_reviewers_have_approved()
 {
    let settings = json!({"do": "approval", "required": {"owners": true, "reviewers": ["userA"]}});
    let reviews = vec![review("userA", "APPROVED", now_milliseconds())];
    let validation = reviewed(reviews)
        .await
        .process_validate("approvals", &settings)
        .await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        first_description(&validation),
        "approval: all required reviewers have approved"
    );
}
