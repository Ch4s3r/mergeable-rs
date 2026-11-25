use crate::contract::case::Stub;
use crate::contract::mock::{MockServer, Scenario};
use anyhow::{Result, anyhow};
use base64::Engine;
use mergeable::actions::Action;
use mergeable::actions::assign::Assign;
use mergeable::actions::checks::{self, Checks};
use mergeable::actions::close::Close;
use mergeable::actions::comment::{self, Comment};
use mergeable::actions::labels::Labels;
use mergeable::actions::merge::Merge;
use mergeable::actions::request_review::RequestReview;
use mergeable::app_state::AppState;
use mergeable::configuration::{self, Configuration, ErrorCode};
use mergeable::context::Context;
use mergeable::error::ErrorKind;
use mergeable::github::api;
use mergeable::github::client::{GithubApp, GithubAppSettings};
use mergeable::metadata;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use rsa::{BoxedUint, PrivateKey};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

const UNUSED_SIGNING_KEY_PRIMES: [&str; 2] = [
    "dcac7fd7aeda8f8b493e39a396e12dda08466a5d059aaab6bcc98a32a98dda1db366e705efbeca2abd0d1a588319d7c8358e77228d3b50e1a09190e618eb02b5",
    "c5bebcc34cdb61de78a8a194915a310297ac2ff838b0391b871464668fc91124284b564d404120b3e96ce14f7930b2a9a4e554659c349da3347af79c8627b3b1",
];
const RSA_PUBLIC_EXPONENT: u64 = 65_537;

fn unused_signing_key_pem() -> Result<String> {
    let [first_prime, second_prime] = UNUSED_SIGNING_KEY_PRIMES.map(|prime_hex| {
        hex::decode(prime_hex).map(|bytes| BoxedUint::from_be_slice_vartime(&bytes))
    });
    let key = PrivateKey::from_p_q(
        first_prime?,
        second_prime?,
        BoxedUint::from(RSA_PUBLIC_EXPONENT),
    )?;
    Ok(key.to_pkcs1_pem(LineEnding::LF)?.to_string())
}

const HOME_REPOSITORY: &str = "owner/repoName";
const REPOSITORY_PATH: &str = "/repos/owner/repoName";
const CALL_WAIT_LIMIT: Duration = Duration::from_secs(5);
const CALL_POLL_INTERVAL: Duration = Duration::from_millis(1);

struct MockedGithub {
    server: MockServer,
    app: Arc<GithubApp>,
}

fn stub(method: &str, path: &str, status: u16, body: &Value) -> Result<Stub> {
    Ok(serde_json::from_value(
        json!({"method": method, "path": path, "status": status, "body": body}),
    )?)
}

fn stub_with_query(
    method: &str,
    path: &str,
    query: &Value,
    status: u16,
    body: &Value,
) -> Result<Stub> {
    Ok(serde_json::from_value(
        json!({"method": method, "path": path, "query": query, "status": status, "body": body}),
    )?)
}

fn repository_path(route: &str) -> String {
    format!("{REPOSITORY_PATH}{route}")
}

impl MockedGithub {
    async fn start(stubs: Vec<Stub>) -> Result<MockedGithub> {
        MockedGithub::start_with_files(stubs, &[]).await
    }

    async fn start_with_files(stubs: Vec<Stub>, files: &[(&str, &str)]) -> Result<MockedGithub> {
        let server = MockServer::start().await?;
        server.github.load_scenario(Scenario {
            home_repository: HOME_REPOSITORY.to_owned(),
            stubs,
            files: files
                .iter()
                .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
                .collect(),
        });
        let settings = GithubAppSettings {
            base_url: format!("http://127.0.0.1:{}/api/v3", server.port),
            app_id: "1".to_owned(),
            private_key_pem: unused_signing_key_pem()?,
            retry_base_delay: Duration::from_millis(1),
            throttle: None,
        };
        let app = Arc::new(GithubApp::new(settings, None)?);
        Ok(MockedGithub { server, app })
    }

    fn context(&self, event_name: &str, payload: Value) -> Context {
        self.context_with(
            event_name,
            payload,
            default_global_settings(),
            Arc::new(AppState::default()),
        )
    }

    fn context_with(
        &self,
        event_name: &str,
        payload: Value,
        global_settings: Value,
        app_state: Arc<AppState>,
    ) -> Context {
        let mut context = Context::new(
            "upstream-test".to_owned(),
            event_name.to_owned(),
            payload,
            self.app.installation(None),
            app_state,
        );
        context.global_settings = Some(Arc::new(global_settings));
        context
    }

    fn recorded_calls(&self) -> Vec<Value> {
        self.server.github.recorded_calls()
    }

    fn recorded_calls_to(&self, method: &str, path: &str) -> Vec<Value> {
        calls_to(&self.recorded_calls(), method, path)
    }

    async fn calls_once_recorded(
        &self,
        is_awaited_call: impl Fn(&Value) -> bool,
        awaited_count: usize,
    ) -> Result<Vec<Value>> {
        let started = Instant::now();
        loop {
            let calls = self.recorded_calls();
            if calls.iter().filter(|call| is_awaited_call(call)).count() >= awaited_count {
                return Ok(calls);
            }
            if started.elapsed() >= CALL_WAIT_LIMIT {
                return Err(anyhow!(
                    "expected {awaited_count} matching calls, recorded only {calls:?}"
                ));
            }
            tokio::time::sleep(CALL_POLL_INTERVAL).await;
        }
    }

    fn forget_calls(&self) {
        self.server.github.take_calls();
    }

    fn stop(self) {
        self.server.stop();
    }
}

fn calls_to(calls: &[Value], method: &str, path: &str) -> Vec<Value> {
    calls
        .iter()
        .filter(|call| call["method"] == method && call["path"] == path)
        .cloned()
        .collect()
}

fn default_global_settings() -> Value {
    json!({
        "use_config_from_pull_request": true,
        "use_config_cache": false,
        "use_org_as_default_config": false
    })
}

fn upstream_mock_payload() -> Value {
    json!({
        "sha": "sha1",
        "action": "opened",
        "repository": {"name": "repoName", "full_name": "fullRepoName", "owner": {"login": "owner"}},
        "sender": {"login": "initiator"},
        "check_suite": {"pull_requests": [{"number": 1}]},
        "pull_request": {
            "user": {"login": "creator"},
            "title": "title",
            "number": 1,
            "created_at": "2024-06-15T19:14:00Z",
            "updated_at": "2024-06-15T19:14:00Z",
            "milestone": null,
            "requested_reviewers": [],
            "requested_teams": [],
            "base": {"repo": {"full_name": "owner/test", "private": false}, "ref": "baseRef", "sha": "sha2"},
            "head": {"ref": "test", "sha": "sha1", "repo": {"full_name": "owner/test", "issues_url": "testRepo/issues{/number}"}},
            "assignees": []
        },
        "issue": {
            "user": {"login": "creator"},
            "title": "title",
            "number": 1,
            "milestone": null,
            "created_at": "2024-06-15T19:14:00Z",
            "updated_at": "2024-06-15T19:14:00Z",
            "assignees": [],
            "pull_request": {}
        }
    })
}

fn scheduled_issues_report(issue_numbers: &[u64]) -> Value {
    let issues: Vec<Value> = issue_numbers
        .iter()
        .map(|number| json!({"number": number, "user": {"login": "scheduler"}}))
        .collect();
    json!({
        "status": "pass",
        "validations": [{"status": "pass", "name": "Label"}],
        "validationSuites": [{"schedule": {"issues": issues, "pulls": []}}]
    })
}

fn passing_label_result() -> Value {
    json!({"status": "pass", "validations": [{"status": "pass", "name": "Label"}]})
}

fn assignee_check_stubs(assignability: &[(&str, u16)]) -> Result<Vec<Stub>> {
    let mut stubs = vec![stub(
        "POST",
        &repository_path("/issues/1/assignees"),
        201,
        &json!({}),
    )?];
    for (assignee, status) in assignability {
        stubs.push(stub(
            "GET",
            &repository_path(&format!("/assignees/{assignee}")),
            *status,
            &Value::Null,
        )?);
    }
    Ok(stubs)
}

async fn added_assignees(
    event_name: &str,
    settings: &Value,
    assignability: &[(&str, u16)],
) -> Result<Vec<Value>> {
    let github = MockedGithub::start(assignee_check_stubs(assignability)?).await?;
    let context = github.context(event_name, upstream_mock_payload());
    Assign
        .after_validate(&context, settings, None, &json!({}))
        .await?;
    let calls = github.recorded_calls_to("POST", &repository_path("/issues/1/assignees"));
    github.stop();
    Ok(calls)
}

async fn assert_assign_is_called_once_for(event_name: &str) -> Result<()> {
    let calls = added_assignees(event_name, &json!({"assignees": []}), &[]).await?;
    assert_eq!(calls.len(), 1);
    Ok(())
}

#[tokio::test]
async fn assign_is_called_for_default_pull_request_events() -> Result<()> {
    assert_assign_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn assign_is_called_for_pull_request_events() -> Result<()> {
    assert_assign_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn assign_is_called_for_issues_events() -> Result<()> {
    assert_assign_is_called_once_for("issues").await
}

#[tokio::test]
async fn assign_is_called_for_issue_comment_events() -> Result<()> {
    assert_assign_is_called_once_for("issue_comment").await
}

#[tokio::test]
async fn assignees_are_added_when_after_validate_is_called_with_proper_parameter() -> Result<()> {
    let calls = added_assignees(
        "pull_request",
        &json!({"assignees": ["testuser1", "testuser2"]}),
        &[("testuser1", 204), ("testuser2", 204)],
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["body"]["assignees"],
        json!(["testuser1", "testuser2"])
    );
    Ok(())
}

#[tokio::test]
async fn creator_is_added_when_assignee_is_author_sender_or_bot() -> Result<()> {
    let calls = added_assignees(
        "pull_request",
        &json!({"assignees": ["@author", "@sender", "@bot"]}),
        &[("initiator", 204), ("Mergeable[bot]", 204)],
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["body"]["assignees"],
        json!(["creator", "initiator", "Mergeable[bot]"])
    );
    Ok(())
}

#[tokio::test]
async fn only_authorized_users_are_added_as_assignee() -> Result<()> {
    let calls = added_assignees(
        "pull_request",
        &json!({"assignees": ["testuser1", "testuser2"]}),
        &[("testuser1", 204), ("testuser2", 404)],
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["body"]["assignees"], json!(["testuser1"]));
    Ok(())
}

async fn closed_issue_updates(
    event_name: &str,
    report: &Value,
    expected_updates: usize,
) -> Result<Vec<Value>> {
    let stubs = (1..=3)
        .map(|number| {
            stub(
                "PATCH",
                &repository_path(&format!("/issues/{number}")),
                200,
                &json!({}),
            )
        })
        .collect::<Result<Vec<Stub>>>()?;
    let github = MockedGithub::start(stubs).await?;
    let context = github.context(event_name, upstream_mock_payload());
    Close
        .after_validate(&context, &json!({}), None, report)
        .await?;
    let calls: Vec<Value> = github
        .calls_once_recorded(|call| call["method"] == "PATCH", expected_updates)
        .await?
        .into_iter()
        .filter(|call| call["method"] == "PATCH")
        .collect();
    github.stop();
    Ok(calls)
}

async fn assert_close_is_called_once_for(event_name: &str) -> Result<()> {
    let calls = closed_issue_updates(event_name, &scheduled_issues_report(&[1]), 1).await?;
    assert_eq!(calls.len(), 1);
    Ok(())
}

#[tokio::test]
async fn close_is_called_for_default_pull_request_events() -> Result<()> {
    assert_close_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn close_is_called_for_pull_request_events() -> Result<()> {
    assert_close_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn close_is_called_for_issues_events() -> Result<()> {
    assert_close_is_called_once_for("issues").await
}

#[tokio::test]
async fn close_is_called_for_issue_comment_events() -> Result<()> {
    assert_close_is_called_once_for("issue_comment").await
}

#[tokio::test]
async fn close_is_called_for_schedule_events() -> Result<()> {
    assert_close_is_called_once_for("schedule").await
}

#[tokio::test]
async fn issue_is_closed() -> Result<()> {
    let calls = closed_issue_updates("pull_request", &json!({}), 1).await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["body"]["state"], "closed");
    Ok(())
}

#[tokio::test]
async fn issues_from_scheduler_are_closed() -> Result<()> {
    let calls = closed_issue_updates("schedule", &scheduled_issues_report(&[1, 2, 3]), 3).await?;
    assert_eq!(calls.len(), 3);
    for call in &calls {
        assert_eq!(call["body"]["state"], "closed");
    }
    Ok(())
}

async fn requested_reviews(
    settings: &Value,
    collaborators: &[&str],
    requested_reviewers: &[&str],
    requested_teams: &[&str],
) -> Result<Vec<Value>> {
    let collaborator_list: Vec<Value> = collaborators
        .iter()
        .map(|login| json!({"login": login}))
        .collect();
    let github = MockedGithub::start(vec![
        stub(
            "GET",
            &repository_path("/collaborators"),
            200,
            &json!(collaborator_list),
        )?,
        stub(
            "POST",
            &repository_path("/pulls/1/requested_reviewers"),
            201,
            &json!({}),
        )?,
    ])
    .await?;
    let mut payload = upstream_mock_payload();
    payload["pull_request"]["requested_reviewers"] = requested_reviewers
        .iter()
        .map(|login| json!({"login": login}))
        .collect();
    payload["pull_request"]["requested_teams"] = requested_teams
        .iter()
        .map(|slug| json!({"slug": slug}))
        .collect();
    let context = github.context("pull_request", payload);
    RequestReview
        .after_validate(&context, settings, None, &passing_label_result())
        .await?;
    let calls = github.recorded_calls_to("POST", &repository_path("/pulls/1/requested_reviewers"));
    github.stop();
    Ok(calls)
}

#[tokio::test]
async fn user_is_requested_a_review_if_user_is_a_collaborator() -> Result<()> {
    let calls = requested_reviews(
        &json!({"reviewers": ["shine2lay"]}),
        &["shine2lay"],
        &[],
        &[],
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["body"]["reviewers"][0], "shine2lay");
    Ok(())
}

#[tokio::test]
async fn author_is_removed_from_list_of_reviewers_to_request() -> Result<()> {
    let calls = requested_reviews(
        &json!({"reviewers": ["creator", "shine2lay"]}),
        &["shine2lay"],
        &[],
        &[],
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["body"]["reviewers"][0], "shine2lay");
    Ok(())
}

#[tokio::test]
async fn requested_reviewers_are_not_requested_again() -> Result<()> {
    let calls = requested_reviews(
        &json!({"reviewers": ["shine2lay"]}),
        &[],
        &["shine2lay"],
        &[],
    )
    .await?;
    assert_eq!(calls.len(), 0);
    Ok(())
}

#[tokio::test]
async fn non_collaborator_is_not_requested_reviews() -> Result<()> {
    let calls = requested_reviews(&json!({"reviewers": ["shine2lay"]}), &[], &[], &[]).await?;
    assert_eq!(calls.len(), 0);
    Ok(())
}

#[tokio::test]
async fn requested_teams_are_not_requested_again() -> Result<()> {
    let calls =
        requested_reviews(&json!({"teams": ["developers"]}), &[], &[], &["developers"]).await?;
    assert_eq!(calls.len(), 0);
    Ok(())
}

#[tokio::test]
async fn requested_team_reviewers_are_added() -> Result<()> {
    let calls = requested_reviews(
        &json!({"reviewers": ["shine2lay"], "teams": ["developers"]}),
        &[],
        &[],
        &["justice-league"],
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["body"]["team_reviewers"][0], "developers");
    Ok(())
}

const CHECK_RUN_ID: &str = "3";

async fn github_for_checks() -> Result<MockedGithub> {
    MockedGithub::start(vec![
        stub(
            "POST",
            &repository_path("/check-runs"),
            201,
            &json!({"id": CHECK_RUN_ID}),
        )?,
        stub(
            "PATCH",
            &repository_path(&format!("/check-runs/{CHECK_RUN_ID}")),
            200,
            &json!({}),
        )?,
        stub(
            "GET",
            &repository_path("/pulls/1"),
            200,
            &json!({"head": {"ref": "test", "sha": "sha1"}}),
        )?,
    ])
    .await
}

fn checks_payload() -> Value {
    let mut payload = upstream_mock_payload();
    payload["action"] = json!("actionName");
    payload
}

async fn checks_with_started_run(
    github: &MockedGithub,
    context: &Context,
    rule_name: Option<&Value>,
) -> Result<Checks> {
    let checks = Checks::default();
    checks
        .before_validate(context, &json!({}), rule_name)
        .await?;
    github.forget_calls();
    Ok(checks)
}

async fn check_run_updates(
    event_name: &str,
    settings: &Value,
    rule_name: Option<&Value>,
    report: &Value,
) -> Result<Vec<Value>> {
    let github = github_for_checks().await?;
    let context = github.context(event_name, checks_payload());
    let checks = checks_with_started_run(&github, &context, rule_name).await?;
    checks
        .after_validate(&context, settings, rule_name, report)
        .await?;
    let calls = github.recorded_calls_to(
        "PATCH",
        &repository_path(&format!("/check-runs/{CHECK_RUN_ID}")),
    );
    github.stop();
    Ok(calls)
}

async fn assert_checks_update_is_called_once_for(event_name: &str) -> Result<()> {
    let settings = json!({"payload": {}, "state": "completed", "status": "success"});
    let calls = check_run_updates(event_name, &settings, None, &json!({})).await?;
    assert_eq!(calls.len(), 1);
    Ok(())
}

#[tokio::test]
async fn checks_is_called_for_default_pull_request_events() -> Result<()> {
    assert_checks_update_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn checks_is_called_for_pull_request_events() -> Result<()> {
    assert_checks_update_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn checks_is_called_for_pull_request_review_events() -> Result<()> {
    assert_checks_update_is_called_once_for("pull_request_review").await
}

#[tokio::test]
async fn checks_is_called_for_issue_comment_events() -> Result<()> {
    assert_checks_update_is_called_once_for("issue_comment").await
}

#[tokio::test]
async fn run_calls_create_api() -> Result<()> {
    let github = github_for_checks().await?;
    let context = github.context("pull_request", checks_payload());
    checks::run(&context, json!({})).await?;
    let calls = github.recorded_calls_to("POST", &repository_path("/check-runs"));
    github.stop();
    assert_eq!(calls.len(), 1);
    Ok(())
}

#[tokio::test]
async fn checks_are_created_when_before_validate_is_called_with_proper_parameter() -> Result<()> {
    let github = github_for_checks().await?;
    let context = github.context("pull_request", checks_payload());
    Checks::default()
        .before_validate(&context, &json!({"name": "test"}), None)
        .await?;
    let calls = github.recorded_calls_to("POST", &repository_path("/check-runs"));
    github.stop();
    assert_eq!(calls.len(), 1);
    Ok(())
}

#[tokio::test]
async fn before_validate_stores_the_name_correctly() -> Result<()> {
    let github = github_for_checks().await?;
    let context = github.context("pull_request", checks_payload());
    let rule_name = json!("test recipe");
    let settings = json!({"payload": {
        "title": "Your run has returned the following status: {{status}}",
        "summary": "This is the summary"
    }});
    let checks = Checks::default();
    checks
        .before_validate(&context, &settings, Some(&rule_name))
        .await?;
    let created = github.recorded_calls_to("POST", &repository_path("/check-runs"));
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["body"]["name"], "Mergeable: test recipe");
    checks
        .after_validate(
            &context,
            &settings,
            Some(&rule_name),
            &passing_label_result(),
        )
        .await?;
    let updated_stored_run = github.recorded_calls_to(
        "PATCH",
        &repository_path(&format!("/check-runs/{CHECK_RUN_ID}")),
    );
    github.stop();
    assert_eq!(updated_stored_run.len(), 1);
    Ok(())
}

#[tokio::test]
async fn conclusion_and_completed_at_are_not_set_when_state_is_not_completed() -> Result<()> {
    let report = json!({"errorCount": 0, "failCount": 1, "failures": [{"name": "approvals"}]});
    let settings = json!({
        "state": "in_progress",
        "status": null,
        "payload": {"title": "This is the title", "summary": "This is the summary"}
    });
    let calls = check_run_updates("pull_request", &settings, None, &report).await?;
    assert_eq!(calls.len(), 1);
    let update = &calls[0]["body"];
    assert_eq!(update["status"], "in_progress");
    assert!(update.get("conclusion").is_none());
    assert!(update.get("completed_at").is_none());
    Ok(())
}

#[tokio::test]
async fn after_validate_output_is_correct() -> Result<()> {
    let settings = json!({"payload": {
        "title": "Your run has returned the following status: {{status}}",
        "summary": "This is the summary"
    }});
    let calls = check_run_updates("pull_request", &settings, None, &passing_label_result()).await?;
    assert_eq!(calls.len(), 1);
    let output = &calls[0]["body"]["output"];
    assert_eq!(output["summary"], "This is the summary");
    assert_eq!(
        output["title"],
        "Your run has returned the following status: pass"
    );
    assert!(!metadata::exists(output["text"].as_str()));
    Ok(())
}

#[tokio::test]
async fn after_validate_replaces_special_annotations_in_payload() -> Result<()> {
    let report = json!({
        "status": "pass",
        "validations": [{"status": "pass", "name": "Label"}],
        "completed_at": "2024-06-15T19:14:00Z"
    });
    let settings = json!({"payload": {
        "title": "@author @sender @bot @repository @action {{formatDate completed_at}} , completed!",
        "summary": "@author @sender @bot @repository @action {{formatDate completed_at}} , summary",
        "text": "@author @sender @bot @repository @action {{formatDate completed_at}} , text"
    }});
    let calls = check_run_updates("pull_request", &settings, None, &report).await?;
    assert_eq!(calls.len(), 1);
    let output = &calls[0]["body"]["output"];
    assert_eq!(
        output["title"],
        "creator initiator Mergeable[bot] fullRepoName actionName Jun 15, 2024, 7:14 PM , completed!"
    );
    assert_eq!(
        output["summary"],
        "creator initiator Mergeable[bot] fullRepoName actionName Jun 15, 2024, 7:14 PM , summary"
    );
    let text = output["text"].as_str().unwrap_or_default();
    assert!(text.contains(
        "creator initiator Mergeable[bot] fullRepoName actionName Jun 15, 2024, 7:14 PM , text"
    ));
    assert!(metadata::exists(Some(text)));
    Ok(())
}

fn failing_check_settings() -> Value {
    json!({"payload": {
        "title": "Your run has returned the following status: {{status}}",
        "summary": "This is the summary",
        "text": "Errors occured."
    }})
}

fn failing_label_result() -> Value {
    json!({"status": "fail", "validations": [{"status": "fail", "name": "Label"}]})
}

#[tokio::test]
async fn after_validate_is_correct_when_validation_fails() -> Result<()> {
    let calls = check_run_updates(
        "pull_request",
        &failing_check_settings(),
        None,
        &failing_label_result(),
    )
    .await?;
    assert_eq!(calls.len(), 1);
    let output = &calls[0]["body"]["output"];
    assert_eq!(output["summary"], "This is the summary");
    assert_eq!(
        output["title"],
        "Your run has returned the following status: fail"
    );
    assert!(metadata::exists(output["text"].as_str()));
    Ok(())
}

#[tokio::test]
async fn correct_name_is_used_in_after_validate_payload() -> Result<()> {
    let rule_name = json!("test recipe");
    let calls = check_run_updates(
        "pull_request",
        &failing_check_settings(),
        Some(&rule_name),
        &failing_label_result(),
    )
    .await?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["body"]["name"], "Mergeable: test recipe");
    assert!(metadata::exists(
        calls[0]["body"]["output"]["text"].as_str()
    ));
    Ok(())
}

struct PullRequestState {
    already_merged: bool,
    details: Value,
}

impl PullRequestState {
    fn is_open_and_unblocked(&self) -> bool {
        self.details["state"] == "open" && self.details["mergeable_state"] != "blocked"
    }
}

fn open_clean_pull_request() -> PullRequestState {
    PullRequestState {
        already_merged: false,
        details: json!({"mergeable_state": "clean", "state": "open"}),
    }
}

fn open_clean_pull_request_with_text(body: &str) -> PullRequestState {
    PullRequestState {
        already_merged: false,
        details: json!({"mergeable_state": "clean", "state": "open", "title": "some pr", "body": body, "number": 10}),
    }
}

async fn merge_github(
    pull_request: &PullRequestState,
    extra_stubs: Vec<Stub>,
) -> Result<MockedGithub> {
    let merged_status = if pull_request.already_merged {
        204
    } else {
        404
    };
    let mut stubs = vec![
        stub(
            "GET",
            &repository_path("/pulls/1/merge"),
            merged_status,
            &Value::Null,
        )?,
        stub(
            "GET",
            &repository_path("/pulls/1"),
            200,
            &pull_request.details,
        )?,
        stub(
            "PUT",
            &repository_path("/pulls/1/merge"),
            200,
            &json!({"merged": true}),
        )?,
    ];
    stubs.extend(extra_stubs);
    MockedGithub::start(stubs).await
}

async fn merge_requests(
    event_name: &str,
    settings: &Value,
    pull_request: &PullRequestState,
) -> Result<Vec<Value>> {
    let github = merge_github(pull_request, Vec::new()).await?;
    let context = github.context(event_name, upstream_mock_payload());
    Merge
        .after_validate(&context, settings, None, &json!({}))
        .await?;
    let merge_path = repository_path("/pulls/1/merge");
    if !pull_request.already_merged {
        github
            .calls_once_recorded(
                |call| call["method"] == "GET" && call["path"] == repository_path("/pulls/1"),
                1,
            )
            .await?;
        if pull_request.is_open_and_unblocked() {
            github
                .calls_once_recorded(
                    |call| call["method"] == "PUT" && call["path"] == merge_path,
                    1,
                )
                .await?;
        } else {
            api::fetch_pull_request(&context, &json!(1)).await?;
        }
    }
    let calls = github.recorded_calls_to("PUT", &merge_path);
    github.stop();
    Ok(calls)
}

#[tokio::test]
async fn merge_is_called_for_status_events() -> Result<()> {
    let search_results = json!({"items": [{"pull_request": true, "number": 1}, {"pull_request": false, "number": 2}]});
    let github = merge_github(
        &open_clean_pull_request(),
        vec![stub("GET", "/search/issues", 200, &search_results)?],
    )
    .await?;
    let context = github.context("status", upstream_mock_payload());
    Merge
        .after_validate(&context, &json!({}), None, &json!({}))
        .await?;
    let calls = github
        .calls_once_recorded(
            |call| call["method"] == "PUT" && call["path"] == repository_path("/pulls/1/merge"),
            1,
        )
        .await?;
    github.stop();
    assert_eq!(calls_to(&calls, "GET", "/search/issues").len(), 1);
    let merges = calls_to(&calls, "PUT", &repository_path("/pulls/1/merge"));
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0]["body"]["merge_method"], "merge");
    Ok(())
}

async fn assert_merge_is_called_once_for(event_name: &str) -> Result<()> {
    let merges = merge_requests(event_name, &json!({}), &open_clean_pull_request()).await?;
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0]["body"]["merge_method"], "merge");
    Ok(())
}

#[tokio::test]
async fn merge_is_called_for_default_pull_request_events() -> Result<()> {
    assert_merge_is_called_once_for("pull_request").await
}

#[tokio::test]
async fn merge_is_called_for_pull_request_review_events() -> Result<()> {
    assert_merge_is_called_once_for("pull_request_review").await
}

#[tokio::test]
async fn merge_is_called_for_check_suite_events() -> Result<()> {
    assert_merge_is_called_once_for("check_suite").await
}

#[tokio::test]
async fn merge_is_called_for_issue_comment_events() -> Result<()> {
    assert_merge_is_called_once_for("issue_comment").await
}

#[tokio::test]
async fn merge_is_not_called_if_pull_request_is_in_a_blocked_state() -> Result<()> {
    let blocked = PullRequestState {
        already_merged: false,
        details: json!({"mergeable_state": "blocked", "state": "open"}),
    };
    assert_eq!(
        merge_requests("pull_request", &json!({}), &blocked)
            .await?
            .len(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn merge_is_not_called_if_pull_request_is_closed() -> Result<()> {
    let closed = PullRequestState {
        already_merged: false,
        details: json!({"mergeable_state": "clean", "state": "closed"}),
    };
    assert_eq!(
        merge_requests("pull_request", &json!({}), &closed)
            .await?
            .len(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn merge_is_not_called_if_pull_request_is_already_merged() -> Result<()> {
    let merged = PullRequestState {
        already_merged: true,
        details: json!({"mergeable_state": "clean", "state": "open"}),
    };
    assert_eq!(
        merge_requests("pull_request", &json!({}), &merged)
            .await?
            .len(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn merge_method_option_works_correctly() -> Result<()> {
    let merges = merge_requests(
        "pull_request",
        &json!({"merge_method": "squash"}),
        &open_clean_pull_request(),
    )
    .await?;
    assert_eq!(merges.len(), 1);
    let body = &merges[0]["body"];
    assert_eq!(body["merge_method"], "squash");
    assert!(body.get("commit_title").is_none());
    assert!(body.get("commit_message").is_none());
    Ok(())
}

#[tokio::test]
async fn commit_title_and_commit_message_options_work_correctly() -> Result<()> {
    let merges = merge_requests(
        "pull_request",
        &json!({"merge_method": "squash", "commit_title": "hello world", "commit_message": "foobar"}),
        &open_clean_pull_request(),
    )
    .await?;
    assert_eq!(merges.len(), 1);
    let body = &merges[0]["body"];
    assert_eq!(body["merge_method"], "squash");
    assert_eq!(body["commit_title"], "hello world");
    assert_eq!(body["commit_message"], "foobar");
    Ok(())
}

#[tokio::test]
async fn commit_title_and_commit_message_options_support_templating() -> Result<()> {
    let merges = merge_requests(
        "pull_request",
        &json!({"merge_method": "squash", "commit_title": "{{{ title }}} (#{{{ number }}})", "commit_message": "{{{ body }}}"}),
        &open_clean_pull_request_with_text("some message"),
    )
    .await?;
    assert_eq!(merges.len(), 1);
    let body = &merges[0]["body"];
    assert_eq!(body["merge_method"], "squash");
    assert_eq!(body["commit_title"], "some pr (#10)");
    assert_eq!(body["commit_message"], "some message");
    Ok(())
}

#[tokio::test]
async fn commit_title_and_commit_message_options_support_empty_string() -> Result<()> {
    let merges = merge_requests(
        "pull_request",
        &json!({"merge_method": "squash", "commit_title": "", "commit_message": ""}),
        &open_clean_pull_request_with_text("some message"),
    )
    .await?;
    assert_eq!(merges.len(), 1);
    let body = &merges[0]["body"];
    assert_eq!(body["merge_method"], "squash");
    assert_eq!(body["commit_title"], "");
    assert_eq!(body["commit_message"], "");
    Ok(())
}

#[tokio::test]
async fn commit_title_and_commit_message_options_do_not_escape_html() -> Result<()> {
    let merges = merge_requests(
        "pull_request",
        &json!({"merge_method": "squash", "commit_title": "", "commit_message": "{{{ body }}}"}),
        &open_clean_pull_request_with_text("some <> ' message"),
    )
    .await?;
    assert_eq!(merges.len(), 1);
    let body = &merges[0]["body"];
    assert_eq!(body["merge_method"], "squash");
    assert_eq!(body["commit_title"], "");
    assert_eq!(body["commit_message"], "some <> ' message");
    Ok(())
}

#[derive(Default)]
struct LabelUpdates {
    replaced: Vec<Value>,
    added: Vec<Value>,
}

async fn label_github(current_labels: &[&str]) -> Result<MockedGithub> {
    let label_list: Vec<Value> = current_labels
        .iter()
        .map(|name| json!({"name": name}))
        .collect();
    let mut stubs = Vec::new();
    for issue_number in 1..=3 {
        let labels_path = repository_path(&format!("/issues/{issue_number}/labels"));
        stubs.push(stub("GET", &labels_path, 200, &json!(label_list))?);
        stubs.push(stub("PUT", &labels_path, 200, &json!([]))?);
        stubs.push(stub("POST", &labels_path, 200, &json!([]))?);
    }
    MockedGithub::start(stubs).await
}

fn is_labels_call(call: &Value, method: &str) -> bool {
    call["method"] == method
        && call["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("/labels"))
}

async fn label_updates(
    event_name: &str,
    current_labels: &[&str],
    settings: &Value,
    report: &Value,
    expected_updates: usize,
) -> Result<LabelUpdates> {
    let github = label_github(current_labels).await?;
    let context = github.context(event_name, upstream_mock_payload());
    Labels
        .after_validate(&context, settings, None, report)
        .await?;
    let calls = github
        .calls_once_recorded(
            |call| is_labels_call(call, "PUT") || is_labels_call(call, "POST"),
            expected_updates,
        )
        .await?;
    github.stop();
    Ok(LabelUpdates {
        replaced: calls
            .iter()
            .filter(|call| is_labels_call(call, "PUT"))
            .cloned()
            .collect(),
        added: calls
            .iter()
            .filter(|call| is_labels_call(call, "POST"))
            .cloned()
            .collect(),
    })
}

async fn labels_of_single_update(current_labels: &[&str], settings: &Value) -> Result<Value> {
    let updates = label_updates("pull_request", current_labels, settings, &json!({}), 1).await?;
    assert_eq!(updates.replaced.len(), 1);
    Ok(updates.replaced[0]["body"]["labels"].clone())
}

async fn assert_labels_are_set_once_for(event_name: &str) -> Result<()> {
    let updates = label_updates(
        event_name,
        &[],
        &json!({"add": ["a label"]}),
        &scheduled_issues_report(&[1]),
        1,
    )
    .await?;
    assert_eq!(updates.replaced.len(), 1);
    Ok(())
}

#[tokio::test]
async fn labels_are_set_for_default_pull_request_events() -> Result<()> {
    assert_labels_are_set_once_for("pull_request").await
}

#[tokio::test]
async fn labels_are_set_for_pull_request_events() -> Result<()> {
    assert_labels_are_set_once_for("pull_request").await
}

#[tokio::test]
async fn labels_are_set_for_issues_events() -> Result<()> {
    assert_labels_are_set_once_for("issues").await
}

#[tokio::test]
async fn labels_are_set_for_issue_comment_events() -> Result<()> {
    assert_labels_are_set_once_for("issue_comment").await
}

#[tokio::test]
async fn labels_are_set_for_schedule_events() -> Result<()> {
    assert_labels_are_set_once_for("schedule").await
}

#[tokio::test]
async fn replace_replaces_existing_labels() -> Result<()> {
    let labels = labels_of_single_update(
        &["drop_label"],
        &json!({"replace": ["work in progress", "development"]}),
    )
    .await?;
    assert_eq!(labels, json!(["work in progress", "development"]));
    Ok(())
}

#[tokio::test]
async fn add_appends_to_existing_labels() -> Result<()> {
    let labels = labels_of_single_update(
        &["another label", "test label"],
        &json!({"add": ["production", "deploy"]}),
    )
    .await?;
    assert_eq!(
        labels,
        json!(["another label", "test label", "production", "deploy"])
    );
    Ok(())
}

#[tokio::test]
async fn delete_removes_from_existing_labels() -> Result<()> {
    let labels = labels_of_single_update(
        &[
            "another label",
            "test label",
            "delete me",
            "delete this too",
        ],
        &json!({"delete": ["delete me", "delete this too"]}),
    )
    .await?;
    assert_eq!(labels, json!(["another label", "test label"]));
    Ok(())
}

#[tokio::test]
async fn label_operations_run_in_order_replace_add_delete() -> Result<()> {
    let labels = labels_of_single_update(
        &["original label", "another label"],
        &json!({
            "delete": ["not present", "more adds"],
            "add": ["addition", "more adds"],
            "replace": ["test present", "not present"]
        }),
    )
    .await?;
    assert_eq!(labels, json!(["test present", "addition"]));
    Ok(())
}

#[tokio::test]
async fn label_operation_settings_can_be_a_single_value() -> Result<()> {
    let labels = labels_of_single_update(
        &["original label", "another label", "delete me"],
        &json!({"delete": "deletion", "add": "addition", "replace": "replace"}),
    )
    .await?;
    assert_eq!(labels, json!(["replace", "addition"]));
    Ok(())
}

#[tokio::test]
async fn unknown_label_mode_throws_an_unsupported_setting_error() -> Result<()> {
    let github = label_github(&[]).await?;
    let context = github.context("pull_request", upstream_mock_payload());
    let outcome = Labels
        .after_validate(
            &context,
            &json!({"labels": ["testLabel", "2ndTestLabel"], "mode": "DNE"}),
            None,
            &json!({}),
        )
        .await;
    github.stop();
    let Err(error) = outcome else {
        return Err(anyhow!("an unknown mode was accepted"));
    };
    assert_eq!(error.kind(), ErrorKind::UnsupportedSetting);
    Ok(())
}

#[tokio::test]
async fn labels_setting_accepts_an_array() -> Result<()> {
    let updates = label_updates(
        "pull_request",
        &[],
        &json!({"labels": ["testLabel", "2ndTestLabel"], "mode": "add"}),
        &json!({}),
        1,
    )
    .await?;
    assert_eq!(updates.added.len(), 1);
    assert_eq!(
        updates.added[0]["body"]["labels"],
        json!(["testLabel", "2ndTestLabel"])
    );
    Ok(())
}

#[tokio::test]
async fn labels_setting_defaults_to_add_mode() -> Result<()> {
    let updates = label_updates(
        "pull_request",
        &[],
        &json!({"labels": ["testLabel", "2ndTestLabel"]}),
        &json!({}),
        1,
    )
    .await?;
    assert_eq!(updates.added.len(), 1);
    assert_eq!(
        updates.added[0]["body"]["labels"],
        json!(["testLabel", "2ndTestLabel"])
    );
    Ok(())
}

#[tokio::test]
async fn replace_mode_sets_the_labels() -> Result<()> {
    let labels = labels_of_single_update(
        &[],
        &json!({"labels": ["testLabel", "2ndTestLabel"], "mode": "replace"}),
    )
    .await?;
    assert_eq!(labels, json!(["testLabel", "2ndTestLabel"]));
    Ok(())
}

const LABELS_WITH_GLOB_MATCHES: [&str; 7] = [
    "testLabel",
    "2ndTestLabel",
    "test1",
    "test2",
    "label1",
    "Label2",
    "anotherLabel2",
];

#[tokio::test]
async fn delete_mode_removes_labels_matching_globs() -> Result<()> {
    let labels = labels_of_single_update(
        &LABELS_WITH_GLOB_MATCHES,
        &json!({"labels": ["*Label*", "test1"], "mode": "delete"}),
    )
    .await?;
    assert_eq!(labels, json!(["test2", "label1"]));
    Ok(())
}

#[tokio::test]
async fn issues_from_scheduler_are_labelled() -> Result<()> {
    let updates = label_updates(
        "schedule",
        &[],
        &json!({"labels": ["testLabel", "2ndTestLabel"]}),
        &scheduled_issues_report(&[1, 2, 3]),
        3,
    )
    .await?;
    assert_eq!(updates.added.len(), 3);
    for call in &updates.added {
        assert_eq!(call["body"]["labels"], json!(["testLabel", "2ndTestLabel"]));
    }
    Ok(())
}

#[tokio::test]
async fn labels_from_scheduler_are_deleted() -> Result<()> {
    let updates = label_updates(
        "schedule",
        &LABELS_WITH_GLOB_MATCHES,
        &json!({"labels": ["*Label*", "test1"], "mode": "delete"}),
        &scheduled_issues_report(&[1, 2, 3]),
        3,
    )
    .await?;
    assert_eq!(updates.replaced.len(), 3);
    for call in &updates.replaced {
        assert_eq!(call["body"]["labels"], json!(["test2", "label1"]));
    }
    Ok(())
}

#[tokio::test]
async fn labels_from_scheduler_are_set() -> Result<()> {
    let updates = label_updates(
        "schedule",
        &[],
        &json!({"labels": ["testLabel", "2ndTestLabel"], "mode": "replace"}),
        &scheduled_issues_report(&[1, 2, 3]),
        3,
    )
    .await?;
    assert_eq!(updates.replaced.len(), 3);
    for call in &updates.replaced {
        assert_eq!(call["body"]["labels"], json!(["testLabel", "2ndTestLabel"]));
    }
    Ok(())
}

const STATUS_COMMENT_BODY: &str = "Your run has returned the following status: {{status}}";

#[derive(Default)]
struct CommentTraffic {
    created: Vec<Value>,
    deleted: Vec<Value>,
}

async fn comment_github(existing_comments: &Value) -> Result<MockedGithub> {
    let mut stubs = Vec::new();
    for issue_number in 1..=3 {
        let comments_path = repository_path(&format!("/issues/{issue_number}/comments"));
        stubs.push(stub("GET", &comments_path, 200, existing_comments)?);
        stubs.push(stub("POST", &comments_path, 201, &json!({}))?);
    }
    for comment_id in 1..=3 {
        stubs.push(stub(
            "DELETE",
            &repository_path(&format!("/issues/comments/{comment_id}")),
            204,
            &Value::Null,
        )?);
    }
    MockedGithub::start(stubs).await
}

fn comment_traffic(calls: &[Value]) -> CommentTraffic {
    CommentTraffic {
        created: calls
            .iter()
            .filter(|call| call["method"] == "POST")
            .cloned()
            .collect(),
        deleted: calls
            .iter()
            .filter(|call| call["method"] == "DELETE")
            .cloned()
            .collect(),
    }
}

async fn comment_after_validate(
    event_name: &str,
    existing_comments: &Value,
    settings: &Value,
    report: &Value,
    expected_comments: usize,
) -> Result<CommentTraffic> {
    let github = comment_github(existing_comments).await?;
    let context = github.context(event_name, upstream_mock_payload());
    Comment
        .after_validate(&context, settings, None, report)
        .await?;
    let calls = github
        .calls_once_recorded(|call| call["method"] == "POST", expected_comments)
        .await?;
    let traffic = comment_traffic(&calls);
    github.stop();
    Ok(traffic)
}

fn deleted_comment_ids(traffic: &CommentTraffic) -> Vec<String> {
    traffic
        .deleted
        .iter()
        .filter_map(|call| call["path"].as_str())
        .filter_map(|path| path.rsplit('/').next())
        .map(str::to_owned)
        .collect()
}

fn status_comment_settings() -> Value {
    json!({"payload": {"body": STATUS_COMMENT_BODY}})
}

fn bot_comments(bot_login: &str) -> Value {
    json!([
        {"id": "1", "user": {"login": "test-1"}},
        {"id": "2", "user": {"login": bot_login}}
    ])
}

fn normal_and_error_comments() -> Value {
    json!([
        {"id": "1", "user": {"login": "test-1"}},
        {"id": "2", "user": {"login": "Mergeable[bot]"}, "body": "This is a normal comment"},
        {"id": "3", "user": {"login": "Mergeable[bot]"}, "body": "This is an Error comment"}
    ])
}

async fn assert_comment_is_created_once_for(event_name: &str) -> Result<()> {
    let traffic = comment_after_validate(
        event_name,
        &json!([]),
        &status_comment_settings(),
        &scheduled_issues_report(&[1]),
        1,
    )
    .await?;
    assert_eq!(traffic.created.len(), 1);
    Ok(())
}

#[tokio::test]
async fn comment_is_called_for_default_pull_request_events() -> Result<()> {
    assert_comment_is_created_once_for("pull_request").await
}

#[tokio::test]
async fn comment_is_called_for_pull_request_events() -> Result<()> {
    assert_comment_is_created_once_for("pull_request").await
}

#[tokio::test]
async fn comment_is_called_for_issues_events() -> Result<()> {
    assert_comment_is_created_once_for("issues").await
}

#[tokio::test]
async fn comment_is_called_for_issue_comment_events() -> Result<()> {
    assert_comment_is_created_once_for("issue_comment").await
}

#[tokio::test]
async fn comment_is_called_for_schedule_events() -> Result<()> {
    assert_comment_is_created_once_for("schedule").await
}

#[tokio::test]
async fn comment_is_created_when_after_validate_is_called_with_proper_parameter() -> Result<()> {
    let traffic = comment_after_validate(
        "pull_request",
        &json!([]),
        &status_comment_settings(),
        &passing_label_result(),
        1,
    )
    .await?;
    assert_eq!(traffic.created.len(), 1);
    assert_eq!(
        traffic.created[0]["body"]["body"],
        "Your run has returned the following status: pass"
    );
    Ok(())
}

#[tokio::test]
async fn comment_is_created_three_times_when_result_contains_three_issues_to_act_on() -> Result<()>
{
    let traffic = comment_after_validate(
        "schedule",
        &json!([]),
        &status_comment_settings(),
        &scheduled_issues_report(&[1, 2, 3]),
        3,
    )
    .await?;
    assert_eq!(traffic.created.len(), 3);
    Ok(())
}

#[tokio::test]
async fn old_comments_from_mergeable_are_deleted_if_they_exist() -> Result<()> {
    let traffic = comment_after_validate(
        "pull_request",
        &bot_comments("Mergeable[bot]"),
        &status_comment_settings(),
        &passing_label_result(),
        1,
    )
    .await?;
    assert_eq!(deleted_comment_ids(&traffic), ["2"]);
    Ok(())
}

#[tokio::test]
async fn old_comments_check_the_lower_cased_bot_name() -> Result<()> {
    let traffic = comment_after_validate(
        "pull_request",
        &bot_comments("mergeable[bot]"),
        &status_comment_settings(),
        &passing_label_result(),
        1,
    )
    .await?;
    assert_eq!(deleted_comment_ids(&traffic), ["2"]);
    Ok(())
}

#[tokio::test]
async fn error_handling_removes_old_error_comments_and_creates_a_new_error_comment() -> Result<()> {
    let github = comment_github(&normal_and_error_comments()).await?;
    let context = github.context("pull_request", upstream_mock_payload());
    comment::handle_error(&context, "This is a new error comment").await?;
    let traffic = comment_traffic(&github.recorded_calls());
    github.stop();
    assert_eq!(deleted_comment_ids(&traffic), ["3"]);
    assert_eq!(
        traffic.created[0]["body"]["body"],
        "This is a new error comment"
    );
    Ok(())
}

#[tokio::test]
async fn remove_error_comments_only_removes_comments_that_include_error() -> Result<()> {
    let github = comment_github(&normal_and_error_comments()).await?;
    let context = github.context("pull_request", upstream_mock_payload());
    comment::remove_error_comments(&context).await?;
    let traffic = comment_traffic(&github.recorded_calls());
    github.stop();
    assert_eq!(deleted_comment_ids(&traffic), ["3"]);
    Ok(())
}

#[tokio::test]
async fn leave_old_comment_option_keeps_old_comments() -> Result<()> {
    let traffic = comment_after_validate(
        "pull_request",
        &bot_comments("Mergeable[bot]"),
        &json!({"payload": {"body": STATUS_COMMENT_BODY}, "leave_old_comment": true}),
        &passing_label_result(),
        1,
    )
    .await?;
    assert_eq!(traffic.deleted.len(), 0);
    Ok(())
}

#[tokio::test]
async fn remove_error_comments_fails_gracefully_if_payload_does_not_exist() -> Result<()> {
    let github = comment_github(&json!([])).await?;
    let context = github.context("pull_request", json!({}));
    comment::remove_error_comments(&context).await?;
    let traffic = comment_traffic(&github.recorded_calls());
    github.stop();
    assert_eq!(traffic.deleted.len(), 0);
    Ok(())
}

#[tokio::test]
async fn special_annotations_are_replaced_in_comments() -> Result<()> {
    let traffic = comment_after_validate(
        "pull_request",
        &json!([]),
        &json!({"payload": {"body": "@author @sender @bot @repository @action {{formatDate created_at}} , do something!"}}),
        &passing_label_result(),
        1,
    )
    .await?;
    assert_eq!(
        traffic.created[0]["body"]["body"],
        "creator initiator Mergeable[bot] fullRepoName opened Jun 15, 2024, 7:14 PM , do something!"
    );
    Ok(())
}

const CONFIG_FILE: &str = ".github/mergeable.yml";

const STALE_ISSUES_AND_PULL_REQUESTS: &str = "
mergeable:
  issues:
    stale:
      days: 20
      message: Issue test
  pull_requests:
    stale:
      days: 20
      message: PR test
";

const STALE_ISSUES_ONLY: &str = "
mergeable:
  issues:
    stale:
      days: 20
      message: Issue test
";

const STALE_ISSUES_AND_PULL_REQUESTS_FROM_HEAD: &str = "
mergeable:
  issues:
    stale:
      days: 20
      message: Issue test
  pull_requests:
    stale:
      days: 20
      message: from HEAD
";

const STALE_ISSUES_FROM_PULL_REQUEST: &str = "
mergeable:
  issues:
    stale:
      days: 20
      message: From PR Config
";

fn parsed_yaml(text: &str) -> Result<Value> {
    mergeable::yaml::parse(text)
        .map_err(|error| anyhow!("{error}"))?
        .ok_or_else(|| anyhow!("empty YAML document"))
}

fn settings_of(text: &str) -> Result<Value> {
    Ok(Configuration::from_settings(parsed_yaml(text)?)?.settings)
}

fn changed_files(files: &[(&str, &str)]) -> Value {
    files
        .iter()
        .map(|(filename, status)| json!({"filename": filename, "status": status}))
        .collect()
}

fn pull_request_config_stub(content: &str) -> Result<Stub> {
    stub_with_query(
        "GET",
        &repository_path(&format!("/contents/{CONFIG_FILE}")),
        &json!({"ref": "sha1"}),
        200,
        &json!({
            "type": "file",
            "encoding": "base64",
            "name": "mergeable.yml",
            "path": CONFIG_FILE,
            "content": base64::engine::general_purpose::STANDARD.encode(content),
        }),
    )
}

struct ConfigRepository<'files> {
    main_branch_config: Option<&'files str>,
    pull_request_config: Option<&'files str>,
    changed_files: Value,
    other_files: Vec<(&'files str, &'files str)>,
}

impl<'files> ConfigRepository<'files> {
    fn with_main_branch_config(main_branch_config: &'files str) -> Self {
        ConfigRepository {
            main_branch_config: Some(main_branch_config),
            pull_request_config: None,
            changed_files: json!([]),
            other_files: Vec::new(),
        }
    }

    async fn start(self) -> Result<MockedGithub> {
        let mut stubs = vec![stub(
            "GET",
            &repository_path("/pulls/1/files"),
            200,
            &self.changed_files,
        )?];
        if let Some(content) = self.pull_request_config {
            stubs.push(pull_request_config_stub(content)?);
        }
        let mut files = self.other_files;
        if let Some(content) = self.main_branch_config {
            files.push((CONFIG_FILE, content));
        }
        MockedGithub::start_with_files(stubs, &files).await
    }
}

fn content_requests(calls: &[Value]) -> usize {
    calls
        .iter()
        .filter(|call| {
            call["path"]
                .as_str()
                .is_some_and(|path| path.contains("/contents/"))
        })
        .count()
}

fn global_settings_with(overrides: &Value) -> Value {
    let mut settings = default_global_settings();
    if let (Some(fields), Some(override_fields)) = (settings.as_object_mut(), overrides.as_object())
    {
        for (key, value) in override_fields {
            fields.insert(key.clone(), value.clone());
        }
    }
    settings
}

#[tokio::test]
async fn bad_yml_in_the_configuration_is_reported_as_bad_yml() -> Result<()> {
    let github = ConfigRepository {
        main_branch_config: None,
        pull_request_config: Some(
            "version: 2\nmergeable:\n  - when: pull_request.*\n     validate: [\n",
        ),
        changed_files: changed_files(&[(CONFIG_FILE, "modified")]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let configuration =
        configuration::load(&github.context("pull_request", upstream_mock_payload())).await;
    github.stop();
    assert_eq!(configuration.errors.len(), 1);
    assert!(configuration.has_error(ErrorCode::BadYml));
    Ok(())
}

#[tokio::test]
async fn missing_yml_is_reported_as_no_yml() -> Result<()> {
    let github = ConfigRepository {
        main_branch_config: None,
        pull_request_config: None,
        changed_files: json!([]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let configuration =
        configuration::load(&github.context("pull_request", upstream_mock_payload())).await;
    github.stop();
    assert_eq!(configuration.errors.len(), 1);
    assert!(configuration.has_error(ErrorCode::NoYml));
    Ok(())
}

#[tokio::test]
async fn config_is_fetched_from_main_branch_if_event_is_not_pull_request() -> Result<()> {
    let github = ConfigRepository::with_main_branch_config(STALE_ISSUES_AND_PULL_REQUESTS)
        .start()
        .await?;
    let configuration =
        configuration::load(&github.context("issues", upstream_mock_payload())).await;
    github.stop();
    assert_eq!(
        configuration.settings,
        settings_of(STALE_ISSUES_AND_PULL_REQUESTS)?
    );
    Ok(())
}

fn caching_global_settings() -> Value {
    global_settings_with(&json!({"use_config_cache": true}))
}

#[tokio::test]
async fn config_cache_is_warmed_and_then_used() -> Result<()> {
    let github = ConfigRepository::with_main_branch_config(STALE_ISSUES_AND_PULL_REQUESTS)
        .start()
        .await?;
    let app_state = Arc::new(AppState::default());
    let context = github.context_with(
        "pull_request",
        upstream_mock_payload(),
        caching_global_settings(),
        Arc::clone(&app_state),
    );
    assert_eq!(app_state.configuration_cache.keys().await.len(), 0);
    assert_eq!(content_requests(&github.recorded_calls()), 0);
    let configuration = configuration::load(&context).await;
    assert_eq!(content_requests(&github.recorded_calls()), 1);
    assert_eq!(
        configuration.settings,
        settings_of(STALE_ISSUES_AND_PULL_REQUESTS)?
    );
    assert_eq!(app_state.configuration_cache.keys().await.len(), 1);
    assert_eq!(
        app_state.configuration_cache.get(HOME_REPOSITORY).await,
        Some(parsed_yaml(STALE_ISSUES_AND_PULL_REQUESTS)?)
    );
    let cached_configuration = configuration::load(&context).await;
    assert_eq!(
        cached_configuration.settings,
        settings_of(STALE_ISSUES_AND_PULL_REQUESTS)?
    );
    assert_eq!(content_requests(&github.recorded_calls()), 1);
    github.stop();
    Ok(())
}

#[tokio::test]
async fn config_cache_is_used_when_already_filled() -> Result<()> {
    let github = ConfigRepository::with_main_branch_config("{}")
        .start()
        .await?;
    let app_state = Arc::new(AppState::default());
    app_state
        .configuration_cache
        .set(
            HOME_REPOSITORY,
            parsed_yaml(STALE_ISSUES_AND_PULL_REQUESTS)?,
        )
        .await;
    let context = github.context_with(
        "pull_request",
        upstream_mock_payload(),
        caching_global_settings(),
        app_state,
    );
    let configuration = configuration::load(&context).await;
    assert_eq!(content_requests(&github.recorded_calls()), 0);
    github.stop();
    assert_eq!(
        configuration.settings,
        settings_of(STALE_ISSUES_AND_PULL_REQUESTS)?
    );
    Ok(())
}

fn push_of_config_file(repository_name: &str) -> Value {
    let mut payload = upstream_mock_payload();
    payload["repository"]["name"] = json!(repository_name);
    payload["head_commit"] = json!({"added": [CONFIG_FILE]});
    payload
}

#[tokio::test]
async fn config_cache_is_invalidated_on_push_events() -> Result<()> {
    let github = ConfigRepository::with_main_branch_config("{}")
        .start()
        .await?;
    let app_state = Arc::new(AppState::default());
    app_state
        .configuration_cache
        .set(
            HOME_REPOSITORY,
            parsed_yaml(STALE_ISSUES_AND_PULL_REQUESTS)?,
        )
        .await;
    let context = github.context_with(
        "push",
        push_of_config_file("repoName"),
        caching_global_settings(),
        Arc::clone(&app_state),
    );
    assert_eq!(content_requests(&github.recorded_calls()), 0);
    let configuration = configuration::load(&context).await;
    assert_eq!(content_requests(&github.recorded_calls()), 1);
    github.stop();
    assert_eq!(configuration.settings, json!({}));
    assert_eq!(app_state.configuration_cache.keys().await.len(), 1);
    Ok(())
}

#[tokio::test]
async fn config_cache_of_the_organization_is_invalidated_on_push_events() -> Result<()> {
    let mut repository = ConfigRepository::with_main_branch_config("{}");
    repository
        .other_files
        .push(("owner/.github:.github/mergeable.yml", "{}"));
    let github = repository.start().await?;
    let app_state = Arc::new(AppState::default());
    let cached = parsed_yaml(STALE_ISSUES_AND_PULL_REQUESTS)?;
    for key in [
        HOME_REPOSITORY,
        "owner/another-repo",
        "owner/yet-another-repo",
    ] {
        app_state.configuration_cache.set(key, cached.clone()).await;
    }
    assert_eq!(app_state.configuration_cache.keys().await.len(), 3);
    let repository_push = github.context_with(
        "push",
        push_of_config_file("repoName"),
        caching_global_settings(),
        Arc::clone(&app_state),
    );
    let configuration = configuration::load(&repository_push).await;
    assert_eq!(content_requests(&github.recorded_calls()), 1);
    assert_eq!(configuration.settings, json!({}));
    assert_eq!(app_state.configuration_cache.keys().await.len(), 3);
    let organization_push = github.context_with(
        "push",
        push_of_config_file(".github"),
        caching_global_settings(),
        Arc::clone(&app_state),
    );
    let configuration = configuration::load(&organization_push).await;
    assert_eq!(content_requests(&github.recorded_calls()), 2);
    github.stop();
    assert_eq!(configuration.settings, json!({}));
    assert_eq!(app_state.configuration_cache.keys().await.len(), 1);
    Ok(())
}

async fn loaded_settings_for_review_events(github: &MockedGithub, payload: &Value) -> Vec<Value> {
    let mut loaded = Vec::new();
    for event_name in ["pull_request", "pull_request_review"] {
        loaded.push(
            configuration::load(&github.context(event_name, payload.clone()))
                .await
                .settings,
        );
    }
    loaded
}

#[tokio::test]
async fn config_is_fetched_from_main_branch_if_the_pull_request_does_not_change_it() -> Result<()> {
    let github = ConfigRepository {
        main_branch_config: Some(STALE_ISSUES_AND_PULL_REQUESTS),
        pull_request_config: Some(STALE_ISSUES_ONLY),
        changed_files: changed_files(&[("someFile", "modified")]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let loaded = loaded_settings_for_review_events(&github, &upstream_mock_payload()).await;
    github.stop();
    let expected = settings_of(STALE_ISSUES_AND_PULL_REQUESTS)?;
    assert_eq!(loaded, [expected.clone(), expected]);
    Ok(())
}

#[tokio::test]
async fn config_is_fetched_from_main_branch_if_the_pull_request_is_from_a_fork() -> Result<()> {
    let github = ConfigRepository {
        main_branch_config: Some(STALE_ISSUES_AND_PULL_REQUESTS),
        pull_request_config: Some(STALE_ISSUES_ONLY),
        changed_files: changed_files(&[(CONFIG_FILE, "modified")]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let mut payload = upstream_mock_payload();
    payload["pull_request"]["head"]["repo"]["full_name"] = json!("owner/not-test");
    let loaded = loaded_settings_for_review_events(&github, &payload).await;
    github.stop();
    let expected = settings_of(STALE_ISSUES_AND_PULL_REQUESTS)?;
    assert_eq!(loaded, [expected.clone(), expected]);
    Ok(())
}

async fn assert_config_is_fetched_from_head_branch_when_changed(
    global_settings: &Value,
) -> Result<()> {
    let expected = settings_of(STALE_ISSUES_FROM_PULL_REQUEST)?;
    let modified = ConfigRepository {
        main_branch_config: Some(STALE_ISSUES_AND_PULL_REQUESTS_FROM_HEAD),
        pull_request_config: Some(STALE_ISSUES_FROM_PULL_REQUEST),
        changed_files: changed_files(&[(CONFIG_FILE, "modified")]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    for event_name in ["pull_request", "pull_request_review"] {
        let context = modified.context_with(
            event_name,
            upstream_mock_payload(),
            global_settings.clone(),
            Arc::new(AppState::default()),
        );
        assert_eq!(configuration::load(&context).await.settings, expected);
    }
    modified.stop();
    let added = ConfigRepository {
        main_branch_config: None,
        pull_request_config: Some(STALE_ISSUES_FROM_PULL_REQUEST),
        changed_files: changed_files(&[(CONFIG_FILE, "added")]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let context = added.context_with(
        "pull_request",
        upstream_mock_payload(),
        global_settings.clone(),
        Arc::new(AppState::default()),
    );
    assert_eq!(configuration::load(&context).await.settings, expected);
    added.stop();
    Ok(())
}

#[tokio::test]
async fn config_is_fetched_from_head_branch_if_the_pull_request_modifies_or_adds_it() -> Result<()>
{
    assert_config_is_fetched_from_head_branch_when_changed(&default_global_settings()).await
}

#[tokio::test]
async fn config_is_fetched_from_head_branch_if_the_pull_request_modifies_or_adds_it_with_cache_enabled()
-> Result<()> {
    assert_config_is_fetched_from_head_branch_when_changed(&caching_global_settings()).await
}

fn find_validator<'rule>(rule_set: &'rule Value, validator_name: &str) -> Option<&'rule Value> {
    rule_set["validate"]
        .as_array()
        .and_then(|validators| validators.iter().find(|item| item["do"] == validator_name))
}

async fn configuration_loaded_with_content_requests(
    main_branch_config: &str,
) -> Result<(Configuration, usize)> {
    let github = ConfigRepository::with_main_branch_config(main_branch_config)
        .start()
        .await?;
    let configuration =
        configuration::load(&github.context("pull_request", upstream_mock_payload())).await;
    let requests = content_requests(&github.recorded_calls());
    github.stop();
    Ok((configuration, requests))
}

#[tokio::test]
async fn loading_with_context_returns_the_right_configuration() -> Result<()> {
    let (configuration, requests) = configuration_loaded_with_content_requests(
        "
mergeable:
  approvals: 5
  label: 'label regex'
  title: 'title regex'
",
    )
    .await?;
    let rule_set = &configuration.settings[0];
    assert_eq!(
        find_validator(rule_set, "approvals").map(|found| &found["min"]["count"]),
        Some(&json!(5))
    );
    assert_eq!(
        find_validator(rule_set, "title").map(|found| &found["must_exclude"]["regex"]),
        Some(&json!("title regex"))
    );
    assert_eq!(
        find_validator(rule_set, "label").map(|found| &found["must_exclude"]["regex"]),
        Some(&json!("label regex"))
    );
    assert_eq!(requests, 1);
    Ok(())
}

#[tokio::test]
async fn loading_with_context_returns_the_right_configuration_for_pull_requests() -> Result<()> {
    let (configuration, requests) = configuration_loaded_with_content_requests(
        "
mergeable:
  pull_requests:
    label: 'label pull regex'
    title: 'title pull regex'
",
    )
    .await?;
    let rule_set = &configuration.settings[0];
    assert_eq!(rule_set["when"], "pull_request.*");
    assert_eq!(
        find_validator(rule_set, "title").map(|found| &found["must_exclude"]["regex"]),
        Some(&json!("title pull regex"))
    );
    assert_eq!(
        find_validator(rule_set, "label").map(|found| &found["must_exclude"]["regex"]),
        Some(&json!("label pull regex"))
    );
    assert_eq!(requests, 1);
    Ok(())
}

#[tokio::test]
async fn loading_with_context_returns_the_right_configuration_for_issues() -> Result<()> {
    let (configuration, requests) = configuration_loaded_with_content_requests(
        "
mergeable:
  issues:
    label: 'label issue regex'
    title: 'title issue regex'
",
    )
    .await?;
    let rule_set = &configuration.settings[0];
    assert_eq!(rule_set["when"], "issues.*");
    assert_eq!(
        find_validator(rule_set, "title").map(|found| &found["must_exclude"]["regex"]),
        Some(&json!("title issue regex"))
    );
    assert_eq!(
        find_validator(rule_set, "label").map(|found| &found["must_exclude"]["regex"]),
        Some(&json!("label issue regex"))
    );
    assert_eq!(requests, 1);
    Ok(())
}

const DEFAULT_STALE_MESSAGE: &str = "There haven't been much activity here. This is stale. Is it still relevant? This is a friendly reminder to please resolve it. :-)";

fn assert_stale_rule_set(rule_set: &Value, expected_message: &str) {
    assert_eq!(rule_set["validate"][0]["do"], "stale");
    assert_eq!(rule_set["validate"][0]["days"], 20);
    assert_eq!(rule_set["pass"][0]["do"], "comment");
    assert_eq!(rule_set["pass"][0]["payload"]["body"], expected_message);
}

#[tokio::test]
async fn loading_with_context_loads_stale_separately_for_pull_requests_and_issues() -> Result<()> {
    let (pull_requests_only, _) = configuration_loaded_with_content_requests(
        "
mergeable:
  pull_requests:
    label: 'label issue regex'
    title: 'title issue regex'
    stale:
      days: 20
",
    )
    .await?;
    assert_stale_rule_set(&pull_requests_only.settings[2], DEFAULT_STALE_MESSAGE);

    let (issues_only, _) = configuration_loaded_with_content_requests(
        "
mergeable:
  issues:
    stale:
      days: 20
",
    )
    .await?;
    assert_stale_rule_set(&issues_only.settings[1], DEFAULT_STALE_MESSAGE);

    let (both, _) =
        configuration_loaded_with_content_requests(STALE_ISSUES_AND_PULL_REQUESTS).await?;
    let issue_rule_set = &both.settings[1];
    let pull_request_rule_set = &both.settings[4];
    assert_eq!(issue_rule_set["when"], "schedule.repository");
    assert_eq!(pull_request_rule_set["when"], "schedule.repository");
    assert_stale_rule_set(issue_rule_set, "Issue test");
    assert_stale_rule_set(pull_request_rule_set, "PR test");
    Ok(())
}

#[tokio::test]
async fn loading_with_context_returns_no_yml_error_if_mergeable_yml_is_not_found() -> Result<()> {
    let github = ConfigRepository {
        main_branch_config: None,
        pull_request_config: None,
        changed_files: json!([]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let configuration =
        configuration::load(&github.context("issues", upstream_mock_payload())).await;
    github.stop();
    assert!(configuration.has_errors());
    assert!(configuration.has_error(ErrorCode::NoYml));
    Ok(())
}

#[tokio::test]
async fn loading_with_context_returns_github_api_error_if_the_pull_request_config_cannot_be_fetched()
-> Result<()> {
    let github = MockedGithub::start(vec![
        stub(
            "GET",
            &repository_path("/pulls/1/files"),
            200,
            &changed_files(&[(CONFIG_FILE, "modified")]),
        )?,
        stub_with_query(
            "GET",
            &repository_path(&format!("/contents/{CONFIG_FILE}")),
            &json!({"ref": "sha1"}),
            403,
            &json!({"message": "Forbidden"}),
        )?,
    ])
    .await?;
    let configuration =
        configuration::load(&github.context("pull_request", upstream_mock_payload())).await;
    github.stop();
    assert!(configuration.has_errors());
    assert!(configuration.has_error(ErrorCode::GithubApiError));
    Ok(())
}

#[tokio::test]
async fn use_config_from_pull_request_setting_selects_the_config_source() -> Result<()> {
    let main_branch_config = "
mergeable:
  issues:
    stale:
      days: 20
      message: From HEAD Config
";
    let github = ConfigRepository {
        main_branch_config: Some(main_branch_config),
        pull_request_config: Some(STALE_ISSUES_FROM_PULL_REQUEST),
        changed_files: changed_files(&[(CONFIG_FILE, "modified")]),
        other_files: Vec::new(),
    }
    .start()
    .await?;
    let load_with = |use_config_from_pull_request: bool| {
        github.context_with(
            "pull_request",
            upstream_mock_payload(),
            global_settings_with(
                &json!({"use_config_from_pull_request": use_config_from_pull_request}),
            ),
            Arc::new(AppState::default()),
        )
    };
    let from_main_branch = configuration::load(&load_with(false)).await;
    let from_pull_request = configuration::load(&load_with(true)).await;
    github.stop();
    assert_eq!(from_main_branch.settings, settings_of(main_branch_config)?);
    assert_eq!(
        from_pull_request.settings,
        settings_of(STALE_ISSUES_FROM_PULL_REQUEST)?
    );
    Ok(())
}

fn rule_set_names(configuration: &Configuration) -> Vec<Value> {
    configuration
        .settings
        .as_array()
        .map(|rule_sets| {
            rule_sets
                .iter()
                .map(|rule_set| rule_set["name"].clone())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn use_org_as_default_config_setting_merges_the_organization_wide_config() -> Result<()> {
    let repository_config = "
version: 2
mergeable:
  - when: pull_request.*, pull_request_review.*
    name: 'repository rules'
    validate:
      - do: title
        must_include:
          regex: 'something'
";
    let organization_config = "
version: 2
mergeable:
  - when: pull_request.*, pull_request_review.*
    name: 'organization rules'
    validate:
      - do: title
        must_include:
          regex: 'nothing'
";
    let mut repository = ConfigRepository::with_main_branch_config(repository_config);
    repository
        .other_files
        .push(("owner/.github:.github/mergeable.yml", organization_config));
    let github = repository.start().await?;
    let with_organization_defaults = github.context_with(
        "pull_request",
        upstream_mock_payload(),
        global_settings_with(&json!({"use_org_as_default_config": true, "use_config_cache": true})),
        Arc::new(AppState::default()),
    );
    let merged = configuration::load(&with_organization_defaults).await;
    let repository_only =
        configuration::load(&github.context("pull_request", upstream_mock_payload())).await;
    github.stop();
    assert_eq!(
        rule_set_names(&merged),
        [json!("organization rules"), json!("repository rules")]
    );
    assert_eq!(
        rule_set_names(&repository_only),
        [json!("repository rules")]
    );
    Ok(())
}
