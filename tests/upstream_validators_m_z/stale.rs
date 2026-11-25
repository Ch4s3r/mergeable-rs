use super::{MockedGithub, PullRequestFields, get_stub, pull_request_payload};
use chrono::{Datelike, Timelike, Utc};
use mergeable::validators::stale::Stale;
use serde_json::{Value, json};

const QUERY_PREFIX: &str = "repo:owner/repo is:open updated:<<timestamp>";
const FAR_EAST_TIME_ZONE: &str = "Pacific/Kiritimati";
const FAR_WEST_TIME_ZONE: &str = "Pacific/Pago_Pago";

fn one_issue_and_one_pull_request() -> Value {
    json!([{"number": 1}, {"number": 2, "pull_request": {}}])
}

async fn github_searching(items: Value) -> MockedGithub {
    MockedGithub::start(
        vec![get_stub("/search/issues", json!({"items": items}))],
        pull_request_payload(PullRequestFields::default()),
    )
    .await
}

fn search_queries(github: &MockedGithub) -> Vec<String> {
    github
        .recorded_calls()
        .iter()
        .filter(|call| call["path"] == "/search/issues")
        .filter_map(|call| call["query"]["q"].as_str().map(str::to_owned))
        .collect()
}

fn queries_with_filter(github: &MockedGithub, filter: &str) -> usize {
    let expected_query = format!("{QUERY_PREFIX} {filter}").trim_end().to_owned();
    search_queries(github)
        .iter()
        .filter(|query| **query == expected_query)
        .count()
}

fn no_query_has(github: &MockedGithub, qualifier: &str) -> bool {
    search_queries(github)
        .iter()
        .all(|query| !query.contains(qualifier))
}

fn scheduled_count(result: &Value, kind: &str) -> Option<usize> {
    result["schedule"][kind].as_array().map(Vec::len)
}

fn assert_one_issue_and_one_pull_request_scheduled(result: &Value) {
    assert_eq!(scheduled_count(result, "issues"), Some(1));
    assert_eq!(scheduled_count(result, "pulls"), Some(1));
    assert_eq!(result["status"], "pass");
}

async fn assert_metadata_queries(draft: bool, milestones: bool, projects: bool) {
    let settings = json!({"do": "stale", "days": 10, "ignore_drafts": draft, "ignore_milestones": milestones, "ignore_projects": projects});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "type:"));
    assert!(no_query_has(&github, "label:"));
    assert!(search_queries(&github).iter().any(|query| {
        query.contains("-is:draft") == draft
            && query.contains("no:milestone") == milestones
            && query.contains("no:project") == projects
    }));
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

fn weekday_in(time_zone: &str) -> String {
    let zone: chrono_tz::Tz = time_zone.parse().expect("known time zone");
    Utc::now().with_timezone(&zone).weekday().to_string()
}

fn current_utc_hour() -> i64 {
    i64::from(Utc::now().hour())
}

#[tokio::test]
async fn both_types_search_issues_and_pull_requests_together() {
    let settings = json!({"do": "stale", "days": 10, "type": ["issues", "pull_request"]});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "type:"));
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

#[tokio::test]
async fn no_type_searches_issues_and_pull_requests_together() {
    let settings = json!({"do": "stale", "days": 10});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "type:"));
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

#[tokio::test]
async fn metadata_query_ignores_drafts_milestones_and_projects() {
    assert_metadata_queries(true, true, true).await;
}

#[tokio::test]
async fn metadata_query_ignores_milestones_and_projects() {
    assert_metadata_queries(false, true, true).await;
}

#[tokio::test]
async fn metadata_query_ignores_drafts_and_projects() {
    assert_metadata_queries(true, false, true).await;
}

#[tokio::test]
async fn metadata_query_ignores_drafts_and_milestones() {
    assert_metadata_queries(true, true, false).await;
}

#[tokio::test]
async fn metadata_query_ignores_projects_only() {
    assert_metadata_queries(false, false, true).await;
}

#[tokio::test]
async fn metadata_query_ignores_drafts_only() {
    assert_metadata_queries(true, false, false).await;
}

#[tokio::test]
async fn metadata_query_ignores_milestones_only() {
    assert_metadata_queries(false, true, false).await;
}

#[tokio::test]
async fn metadata_query_ignores_nothing() {
    assert_metadata_queries(false, false, false).await;
}

#[tokio::test]
async fn label_matches_and_ignores_become_label_qualifiers() {
    let settings = json!({"do": "stale", "days": 10, "label": {"match": ["test_match1", "test_match2"], "ignore": ["test_ignore"]}});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "type:"));
    assert!(!no_query_has(&github, "label:"));
    assert_eq!(
        queries_with_filter(
            &github,
            "label:\"test_match1\" label:\"test_match2\" -label:\"test_ignore\""
        ),
        1
    );
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

#[tokio::test]
async fn label_matches_become_label_qualifiers() {
    let settings =
        json!({"do": "stale", "days": 10, "label": {"match": ["test_match1", "test_match2"]}});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "type:"));
    assert!(!no_query_has(&github, "label:"));
    assert_eq!(
        queries_with_filter(&github, "label:\"test_match1\" label:\"test_match2\""),
        1
    );
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

#[tokio::test]
async fn label_ignores_become_negated_label_qualifiers() {
    let settings =
        json!({"do": "stale", "days": 10, "label": {"ignore": ["test_ignore", "test_ignore2"]}});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "type:"));
    assert!(!no_query_has(&github, "label:"));
    assert_eq!(
        queries_with_filter(&github, "-label:\"test_ignore\" -label:\"test_ignore2\""),
        1
    );
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

#[tokio::test]
async fn no_label_settings_add_no_label_qualifiers() {
    let settings = json!({"do": "stale", "days": 10});
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert!(no_query_has(&github, "label:"));
    assert_one_issue_and_one_pull_request_scheduled(&result);
}

#[tokio::test]
async fn unsupported_types_still_schedule_issues_and_pulls() {
    let github = github_searching(one_issue_and_one_pull_request()).await;
    let only_unsupported = json!({"do": "stale", "days": 10, "type": ["junk1", "junk2"]});
    let result = github.process_validate(&Stale, &only_unsupported).await;
    assert!(no_query_has(&github, "type:"));
    assert!(result["schedule"]["issues"].is_array());
    assert!(result["schedule"]["pulls"].is_array());

    let unsupported_and_issues = json!({"do": "stale", "days": 10, "type": ["junk", "issues"]});
    let result = github
        .process_validate(&Stale, &unsupported_and_issues)
        .await;
    assert_eq!(queries_with_filter(&github, "type:issue"), 1);
    assert!(result["schedule"]["issues"].is_array());
    assert!(result["schedule"]["pulls"].is_array());
}

#[tokio::test]
async fn issues_type_only_searches_issues() {
    let settings = json!({"do": "stale", "days": 10, "type": ["issues"]});
    let github = github_searching(json!([{"number": 1}])).await;
    let result = github.validate(&Stale, &settings).await;
    assert_eq!(queries_with_filter(&github, "type:issue"), 1);
    assert_eq!(queries_with_filter(&github, "type:pr"), 0);
    assert_eq!(scheduled_count(&result, "issues"), Some(1));
    assert_eq!(scheduled_count(&result, "pulls"), Some(0));
    assert_eq!(result["status"], "pass");

    let nothing_stale = github_searching(json!([])).await;
    let result = nothing_stale.process_validate(&Stale, &settings).await;
    assert_eq!(result["status"], "fail");
}

#[tokio::test]
async fn pull_request_type_only_searches_pull_requests() {
    let settings = json!({"do": "stale", "days": 10, "type": ["pull_request"]});
    let github = github_searching(json!([{"number": 1, "pull_request": {}}])).await;
    let result = github.process_validate(&Stale, &settings).await;
    assert_eq!(queries_with_filter(&github, "type:issue"), 0);
    assert_eq!(queries_with_filter(&github, "type:pr"), 1);
    assert_eq!(scheduled_count(&result, "pulls"), Some(1));
    assert_eq!(scheduled_count(&result, "issues"), Some(0));
    assert_eq!(result["status"], "pass");

    let nothing_stale = github_searching(json!([])).await;
    let result = nothing_stale.process_validate(&Stale, &settings).await;
    assert_eq!(result["status"], "fail");
}

#[tokio::test]
async fn time_zone_option_decides_the_local_day() {
    let github = github_searching(json!([{"number": 1, "pull_request": {}}])).await;
    let without_constraint = json!({"do": "stale", "days": 10});
    let result = github.process_validate(&Stale, &without_constraint).await;
    assert_eq!(result["status"], "pass");

    let far_west_day = weekday_in(FAR_WEST_TIME_ZONE);
    let far_west_zone = json!({"do": "stale", "days": 10, "time_constraint": {"time_zone": FAR_WEST_TIME_ZONE, "days_of_week": [far_west_day]}});
    let result = github.process_validate(&Stale, &far_west_zone).await;
    assert_eq!(result["status"], "pass");

    let far_east_zone = json!({"do": "stale", "days": 10, "time_constraint": {"time_zone": FAR_EAST_TIME_ZONE, "days_of_week": [far_west_day]}});
    let result = github.process_validate(&Stale, &far_east_zone).await;
    assert_eq!(result["status"], "fail");
}

#[tokio::test]
async fn days_of_week_option_only_runs_on_listed_days() {
    let github = github_searching(json!([{"number": 1, "pull_request": {}}])).await;
    let today = weekday_in("UTC");
    let other_day = Utc::now().weekday().succ().to_string();

    let other_day_only =
        json!({"do": "stale", "days": 10, "time_constraint": {"days_of_week": [other_day]}});
    let result = github.process_validate(&Stale, &other_day_only).await;
    assert_eq!(result["status"], "fail");

    let today_only =
        json!({"do": "stale", "days": 10, "time_constraint": {"days_of_week": [today]}});
    let result = github.process_validate(&Stale, &today_only).await;
    assert_eq!(result["status"], "pass");
}

#[tokio::test]
async fn hours_between_option_only_runs_within_the_hours() {
    let github = github_searching(json!([{"number": 1, "pull_request": {}}])).await;
    let hour = current_utc_hour();
    let hours_between = |earliest: i64, latest: i64| json!({"do": "stale", "days": 10, "time_constraint": {"hours_between": [earliest.to_string(), latest.to_string()]}});

    let result = github
        .process_validate(&Stale, &hours_between(hour - 1, hour + 9))
        .await;
    assert_eq!(result["status"], "pass");

    let result = github
        .process_validate(&Stale, &hours_between(hour + 1, hour + 9))
        .await;
    assert_eq!(result["status"], "fail");

    let result = github
        .process_validate(&Stale, &hours_between(hour - 7, hour - 1))
        .await;
    assert_eq!(result["status"], "fail");
}
