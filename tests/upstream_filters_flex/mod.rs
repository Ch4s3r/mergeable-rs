use crate::contract::case::Stub;
use crate::contract::mock::{MockServer, Scenario};
use mergeable::app_config::{AppConfig, Environment};
use mergeable::app_state::AppState;
use mergeable::configuration::Configuration;
use mergeable::context::Context;
use mergeable::filters::and_operator::And;
use mergeable::filters::author::Author;
use mergeable::filters::not_operator::Not;
use mergeable::filters::or_operator::Or;
use mergeable::filters::repository::Repository;
use mergeable::filters::{Filter, process_filter};
use mergeable::github::client::{GithubApp, GithubAppSettings};
use mergeable::registry::Registry;
use mergeable::settings::{self, ErrorCode};
use mergeable::{interceptors, metadata, workflow, yaml};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rsa::PrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

const OWNER: &str = "owner";
const REPOSITORY_NAME: &str = "repoName";
const AUTHOR_NAME: &str = "mergeabletestauthorname";
const OTHER_AUTHOR_NAME: &str = "someone-else";
const TEAM: &str = "org/team-slug";
const TEAM_MEMBERS_PATH: &str = "/orgs/org/teams/team-slug/members";
const ORGANIZATION_SETTINGS_FILE: &str = "owner/.github:.github/mergeable.settings.yml";
const ORGANIZATION_SETTINGS_PATH: &str =
    "/repos/owner/.github/contents/.github/mergeable.settings.yml";
const CHECK_RUNS_PATH: &str = "/repos/owner/repoName/check-runs";
const DETACHED_TASK_WAIT: Duration = Duration::from_secs(5);

const FAST_TEST_SIGNING_KEY_SEED: u64 = 0x7570_7374_7265_616d;
const FAST_TEST_SIGNING_KEY_BITS: usize = 1024;

static FAST_TEST_SIGNING_KEY_PEM: LazyLock<String> = LazyLock::new(|| {
    let mut seeded_rng = StdRng::seed_from_u64(FAST_TEST_SIGNING_KEY_SEED);
    PrivateKey::new(&mut seeded_rng, FAST_TEST_SIGNING_KEY_BITS)
        .unwrap()
        .to_pkcs1_pem(LineEnding::LF)
        .unwrap()
        .to_string()
});

fn test_environment(mock_port: u16, extra_variables: &[(&str, &str)]) -> Environment {
    let github_host = format!("127.0.0.1:{mock_port}");
    let mut variables: BTreeMap<&str, &str> = BTreeMap::from([
        ("APP_ID", "1"),
        ("PRIVATE_KEY", FAST_TEST_SIGNING_KEY_PEM.as_str()),
        ("WEBHOOK_SECRET", "test-secret"),
        ("GHE_HOST", github_host.as_str()),
        ("GHE_PROTOCOL", "http"),
        ("GITHUB_THROTTLE", "false"),
        ("GITHUB_RETRY_BASE_MS", "1"),
    ]);
    variables.extend(extra_variables.iter().copied());
    variables.into_iter().collect()
}

struct MockedGithub {
    mock: MockServer,
    config: AppConfig,
}

fn stub(method: &str, path: &str, body: &Value) -> Stub {
    serde_json::from_value(json!({"method": method, "path": path, "status": 200, "body": body}))
        .unwrap()
}

impl MockedGithub {
    async fn start(stubs: Vec<Stub>, files: &[(&str, &str)]) -> MockedGithub {
        MockedGithub::start_with_environment(stubs, files, &[]).await
    }

    async fn start_with_environment(
        stubs: Vec<Stub>,
        files: &[(&str, &str)],
        extra_variables: &[(&str, &str)],
    ) -> MockedGithub {
        let mock = MockServer::start().await.unwrap();
        mock.github.load_scenario(Scenario {
            home_repository: format!("{OWNER}/{REPOSITORY_NAME}"),
            stubs,
            files: files
                .iter()
                .map(|(path, content)| ((*path).to_owned(), (*content).to_owned()))
                .collect(),
        });
        let config =
            AppConfig::from_environment(&test_environment(mock.port, extra_variables)).unwrap();
        MockedGithub { mock, config }
    }

    fn context(&self, event_name: &str, payload: Value) -> Context {
        let github_app = Arc::new(
            GithubApp::new(
                GithubAppSettings {
                    base_url: self.config.github_base_url.clone(),
                    app_id: self.config.app_id.clone(),
                    private_key_pem: self.config.private_key_pem.clone(),
                    retry_base_delay: Duration::from_millis(1),
                    throttle: None,
                    http_timeouts: mergeable::app_config::OutgoingHttpTimeouts::default(),
                },
                None,
            )
            .unwrap(),
        );
        let mut context = Context::new(
            "event-id".to_owned(),
            event_name.to_owned(),
            payload,
            github_app.installation(Some(1)),
            Arc::new(AppState::new(self.config.behavior.clone())),
        );
        context.global_settings = Some(Arc::new(json!({
            "config_path": "",
            "use_config_cache": false,
            "use_org_as_default_config": false,
            "use_config_from_pull_request": true,
        })));
        context
    }

    fn calls_to(&self, method: &str, path: &str) -> Vec<Value> {
        self.mock
            .github
            .recorded_calls()
            .into_iter()
            .filter(|call| call["method"] == method && call["path"] == path)
            .collect()
    }

    fn call_count(&self) -> usize {
        self.mock.github.recorded_call_count()
    }

    async fn wait_for_calls_to(
        &self,
        method: &str,
        path: &str,
        expected_count: usize,
    ) -> Vec<Value> {
        let started = Instant::now();
        loop {
            let calls = self.calls_to(method, path);
            if calls.len() >= expected_count || started.elapsed() > DETACHED_TASK_WAIT {
                return calls;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for MockedGithub {
    fn drop(&mut self) {
        self.mock.github.take_calls();
    }
}

fn pull_request_payload(author: &str) -> Value {
    json!({
        "action": "opened",
        "repository": {"name": REPOSITORY_NAME, "full_name": "fullRepoName", "owner": {"login": OWNER}},
        "sender": {"login": "initiator"},
        "pull_request": {
            "user": {"login": author},
            "title": "title",
            "number": 1,
            "milestone": null,
            "labels": [],
            "base": {"repo": {"full_name": "owner/test", "private": false}, "ref": "baseRef", "sha": "sha2"},
            "head": {"ref": "test", "sha": "sha1", "repo": {"full_name": "owner/test"}},
        },
        "issue": {"user": {"login": "creator"}, "title": "title", "number": 1, "pull_request": {}},
    })
}

fn status_of(result: &Value) -> &str {
    result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn topics_stub(topics: &[&str]) -> Stub {
    stub(
        "GET",
        "/repos/owner/repoName/topics",
        &json!({"names": topics}),
    )
}

async fn filter_with_topics(filter: &dyn Filter, topics: &[&str], settings: Value) -> Value {
    let github = MockedGithub::start(vec![topics_stub(topics)], &[]).await;
    let context = github.context("pull_request", pull_request_payload("creator"));
    process_filter(filter, &context, &settings, &Registry::default())
        .await
        .unwrap()
}

fn topics_must_include(regex: &str) -> Value {
    json!({"do": "repository", "topics": {"must_include": {"regex": regex}}})
}

fn topics_must_exclude(regex: &str) -> Value {
    json!({"do": "repository", "topics": {"must_exclude": {"regex": regex}}})
}

fn misspelled_topics_option(regex: &str) -> Value {
    json!({"do": "repository", "topics": {"must_inclxude": {"regex": regex}}})
}

mod and_filter {
    use super::*;

    #[tokio::test]
    async fn and_runs_its_sub_filters() {
        let settings = json!({"do": "and", "filter": [topics_must_include("Topic 2")]});
        let result = filter_with_topics(&And, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn and_fails_when_the_first_sub_filter_fails_and_the_second_passes() {
        let settings = json!({"do": "and", "filter": [topics_must_include("Topic 2"), topics_must_include("Topic 1")]});
        let result = filter_with_topics(&And, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn and_passes_when_every_sub_filter_passes() {
        let settings = json!({"do": "and", "filter": [topics_must_include("Topic 1"), topics_must_exclude("Topic 2")]});
        let result = filter_with_topics(&And, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn and_without_filter_option_is_an_error() {
        let result = filter_with_topics(&And, &["Topic 1"], json!({"do": "and"})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn and_with_non_list_filter_option_is_an_error() {
        let result =
            filter_with_topics(&And, &["Topic 1"], json!({"do": "and", "filter": ""})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn and_with_empty_filter_option_is_an_error() {
        let result =
            filter_with_topics(&And, &["Topic 1"], json!({"do": "and", "filter": []})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn and_with_unknown_sub_filter_is_an_error() {
        let settings = json!({"do": "and", "filter": [{"do": "missing"}]});
        let result = filter_with_topics(&And, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn and_supports_a_nested_or_filter() {
        let settings = json!({"do": "and", "filter": [
            {"do": "or", "filter": [topics_must_include("Topic 1"), topics_must_include("Topic 2")]},
            topics_must_include("Version 3"),
        ]});
        let result = filter_with_topics(&And, &["Topic 2"], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn and_errors_when_a_nested_sub_filter_errors() {
        let settings = json!({"do": "and", "filter": [
            {"do": "and", "filter": [misspelled_topics_option("Topic 1"), topics_must_include("Topic 2")]},
            topics_must_include("Version 3"),
        ]});
        let result = filter_with_topics(&And, &["Topic 2"], settings).await;
        assert_eq!(status_of(&result), "error");
    }
}

mod or_filter {
    use super::*;

    #[tokio::test]
    async fn or_runs_its_sub_filters() {
        let settings = json!({"do": "or", "filter": [topics_must_include("Topic 2")]});
        let result = filter_with_topics(&Or, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn or_passes_when_the_first_sub_filter_fails_and_the_second_passes() {
        let settings = json!({"do": "or", "filter": [topics_must_include("Topic 2"), topics_must_include("Topic 1")]});
        let result = filter_with_topics(&Or, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn or_passes_when_the_first_sub_filter_passes() {
        let settings = json!({"do": "or", "filter": [topics_must_include("Topic 1"), topics_must_include("Topic 2")]});
        let result = filter_with_topics(&Or, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn or_without_filter_option_is_an_error() {
        let result = filter_with_topics(&Or, &["Topic 1"], json!({"do": "or"})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn or_with_non_list_filter_option_is_an_error() {
        let result = filter_with_topics(&Or, &["Topic 1"], json!({"do": "or", "filter": ""})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn or_with_empty_filter_option_is_an_error() {
        let result = filter_with_topics(&Or, &["Topic 1"], json!({"do": "or", "filter": []})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn or_with_unknown_sub_filter_is_an_error() {
        let settings = json!({"do": "or", "filter": [{"do": "missing"}]});
        let result = filter_with_topics(&Or, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn or_supports_a_nested_or_filter() {
        let settings = json!({"do": "or", "filter": [
            {"do": "or", "filter": [topics_must_include("Topic 1"), topics_must_include("Topic 2")]},
            topics_must_include("Topic 3"),
        ]});
        let result = filter_with_topics(&Or, &["Topic 2"], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn or_errors_when_a_nested_sub_filter_errors() {
        let settings = json!({"do": "or", "filter": [
            {"do": "or", "filter": [
                {"do": "repository", "topics": {"must_incdlude": {"regex": "Topic 1"}}},
                topics_must_include("Topic 2"),
            ]},
            topics_must_include("Topic 3"),
        ]});
        let result = filter_with_topics(&Or, &["Topic 2"], settings).await;
        assert_eq!(status_of(&result), "error");
    }
}

mod not_filter {
    use super::*;

    #[tokio::test]
    async fn not_passes_when_its_sub_filter_fails() {
        let settings = json!({"do": "not", "filter": [topics_must_include("Topic 2")]});
        let result = filter_with_topics(&Not, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn not_fails_when_its_sub_filter_passes() {
        let settings = json!({"do": "not", "filter": [topics_must_include("Topic 1")]});
        let result = filter_with_topics(&Not, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn not_with_more_than_one_sub_filter_is_an_error() {
        let settings = json!({"do": "not", "filter": [topics_must_include("Topic 1"), topics_must_include("Topic 2")]});
        let result = filter_with_topics(&Not, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn not_without_filter_option_is_an_error() {
        let result = filter_with_topics(&Not, &["Topic 1"], json!({"do": "not"})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn not_with_non_list_filter_option_is_an_error() {
        let result =
            filter_with_topics(&Not, &["Topic 1"], json!({"do": "not", "filter": ""})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn not_with_empty_filter_option_is_an_error() {
        let result =
            filter_with_topics(&Not, &["Topic 1"], json!({"do": "not", "filter": []})).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn not_with_unknown_sub_filter_is_an_error() {
        let settings = json!({"do": "not", "filter": [{"do": "missing"}]});
        let result = filter_with_topics(&Not, &["Topic 1"], settings).await;
        assert_eq!(status_of(&result), "error");
    }

    #[tokio::test]
    async fn not_errors_when_a_nested_sub_filter_errors() {
        let settings = json!({"do": "not", "filter": [
            {"do": "not", "filter": [misspelled_topics_option("Topic 1"), topics_must_include("Topic 2")]},
            topics_must_include("Version 3"),
        ]});
        let result = filter_with_topics(&Not, &["Topic 2"], settings).await;
        assert_eq!(status_of(&result), "error");
    }
}

mod repository_filter_topics {
    use super::*;

    #[tokio::test]
    async fn pattern_that_never_matches_passes_must_exclude_gracefully() {
        let result =
            filter_with_topics(&Repository, &["my-topic"], topics_must_exclude("@#$@#$@#$")).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn topic_matching_must_include_passes() {
        let result =
            filter_with_topics(&Repository, &["mytopic"], topics_must_include("mytopic")).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn missing_topic_fails_must_include() {
        let result = filter_with_topics(&Repository, &[], topics_must_include("mytopic")).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn missing_topic_passes_must_exclude() {
        let result = filter_with_topics(&Repository, &[], topics_must_exclude("mytopic")).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn topic_matching_must_exclude_fails() {
        let result =
            filter_with_topics(&Repository, &["mytopic"], topics_must_exclude("mytopic")).await;
        assert_eq!(status_of(&result), "fail");
    }
}

mod author_filter_teams {
    use super::*;

    fn membership_stub(user: &str) -> Stub {
        stub(
            "GET",
            &format!("/orgs/org/teams/team-slug/memberships/{user}"),
            &json!({"state": "active"}),
        )
    }

    fn team_members_stub(members: &[&str]) -> Stub {
        let members: Vec<Value> = members
            .iter()
            .map(|login| json!({"login": login}))
            .collect();
        stub("GET", TEAM_MEMBERS_PATH, &json!(members))
    }

    async fn filter_author(author: &str, stubs: Vec<Stub>, settings: Value) -> Value {
        let github = MockedGithub::start(stubs, &[]).await;
        let context = github.context("pull_request", pull_request_payload(author));
        process_filter(&Author, &context, &settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn expected_author_in_the_team_passes() {
        let settings =
            json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "team": TEAM});
        let result = filter_author(AUTHOR_NAME, vec![membership_stub(AUTHOR_NAME)], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn expected_author_one_of_the_team_members_passes() {
        let settings = json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "one_of": ["@org/team-slug"]});
        let result = filter_author(
            AUTHOR_NAME,
            vec![team_members_stub(&[AUTHOR_NAME])],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn expected_author_outside_the_team_fails() {
        let settings =
            json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "team": TEAM});
        let result = filter_author(AUTHOR_NAME, vec![], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn expected_author_not_one_of_the_empty_team_fails() {
        let settings = json!({"do": "author", "must_include": {"regex": AUTHOR_NAME}, "one_of": ["@org/team-slug"]});
        let result = filter_author(AUTHOR_NAME, vec![team_members_stub(&[])], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn unexpected_author_in_the_team_fails() {
        let settings =
            json!({"do": "author", "must_include": {"regex": OTHER_AUTHOR_NAME}, "team": TEAM});
        let result = filter_author(AUTHOR_NAME, vec![membership_stub(AUTHOR_NAME)], settings).await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn unexpected_author_one_of_the_team_members_fails() {
        let settings = json!({"do": "author", "must_include": {"regex": OTHER_AUTHOR_NAME}, "one_of": ["@org/team-slug"]});
        let result = filter_author(
            AUTHOR_NAME,
            vec![team_members_stub(&[AUTHOR_NAME])],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn author_who_is_a_team_member_passes_the_team_option() {
        let settings = json!({"do": "author", "team": TEAM});
        let result = filter_author(AUTHOR_NAME, vec![membership_stub(AUTHOR_NAME)], settings).await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn author_one_of_the_team_members_passes() {
        let settings = json!({"do": "author", "one_of": ["@org/team-slug"]});
        let result = filter_author(
            AUTHOR_NAME,
            vec![team_members_stub(&[AUTHOR_NAME])],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn author_who_is_not_a_team_member_fails_the_team_option() {
        let settings = json!({"do": "author", "team": TEAM});
        let result = filter_author(
            "mergeable",
            vec![membership_stub(OTHER_AUTHOR_NAME)],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn author_not_one_of_the_team_members_fails() {
        let settings = json!({"do": "author", "one_of": ["@org/team-slug"]});
        let result = filter_author(
            "mergeable",
            vec![team_members_stub(&[OTHER_AUTHOR_NAME])],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }

    #[tokio::test]
    async fn author_outside_the_none_of_team_passes() {
        let settings = json!({"do": "author", "none_of": ["@org/team-slug"]});
        let result = filter_author(
            AUTHOR_NAME,
            vec![team_members_stub(&[OTHER_AUTHOR_NAME])],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "pass");
    }

    #[tokio::test]
    async fn author_in_the_none_of_team_fails() {
        let settings = json!({"do": "author", "none_of": ["@org/team-slug"]});
        let result = filter_author(
            AUTHOR_NAME,
            vec![team_members_stub(&[AUTHOR_NAME])],
            settings,
        )
        .await;
        assert_eq!(status_of(&result), "fail");
    }
}

mod flex_executor {
    use super::*;

    const GREETING_BODY: &str = "Thanks for creating a pull request! A maintainer will review your changes shortly. Please don't be discouraged if it takes a while.";

    fn greeting_config(outcome: &str) -> Configuration {
        let config_text = format!(
            "
version: 2
mergeable:
  - when: pull_request.opened
    name: \"Greet a contributor\"
    validate: []
    {outcome}:
      - do: comment
        payload:
          body: >
            {GREETING_BODY}
"
        );
        Configuration::from_settings(yaml::parse(&config_text).unwrap().unwrap()).unwrap()
    }

    #[tokio::test]
    async fn bad_yml_creates_one_cancelled_check_run() {
        let bad_config = "
      version: 2
        mergeable:
    when: pull_request.*
      ";
        let github = MockedGithub::start(
            vec![
                stub(
                    "GET",
                    "/repos/owner/repoName/pulls/1/files",
                    &json!([{"filename": ".github/mergeable.yml", "status": "modified"}]),
                ),
                stub("POST", CHECK_RUNS_PATH, &json!({"id": 1})),
            ],
            &[(".github/mergeable.yml", bad_config)],
        )
        .await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        workflow::execute(context).await.unwrap();

        assert_eq!(github.calls_to("PATCH", CHECK_RUNS_PATH).len(), 0);
        let created_check_runs = github.calls_to("POST", CHECK_RUNS_PATH);
        assert_eq!(created_check_runs.len(), 1);
        assert_eq!(created_check_runs[0]["body"]["status"], "completed");
        assert_eq!(created_check_runs[0]["body"]["conclusion"], "cancelled");
        assert_eq!(
            created_check_runs[0]["body"]["output"]["title"],
            "Invalid Configuration"
        );
        let summary = created_check_runs[0]["body"]["output"]["summary"]
            .as_str()
            .unwrap_or_default();
        assert!(summary.contains("Invalid YML format"), "{summary}");
    }

    #[tokio::test]
    async fn pass_action_runs_for_a_rule_without_validators() {
        let github = MockedGithub::start(
            vec![stub(
                "GET",
                "/repos/owner/repoName/issues/1/comments",
                &json!([]),
            )],
            &[],
        )
        .await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        workflow::process(&context, &Registry::default(), &greeting_config("pass"))
            .await
            .unwrap();
        let comments = github
            .wait_for_calls_to("POST", "/repos/owner/repoName/issues/1/comments", 1)
            .await;
        assert_eq!(comments.len(), 1);
    }

    #[tokio::test]
    async fn fail_action_does_not_run_for_a_rule_without_validators() {
        let github = MockedGithub::start(
            vec![stub(
                "GET",
                "/repos/owner/repoName/issues/1/comments",
                &json!([]),
            )],
            &[],
        )
        .await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        workflow::process(&context, &Registry::default(), &greeting_config("fail"))
            .await
            .unwrap();
        assert_eq!(
            github
                .calls_to("POST", "/repos/owner/repoName/issues/1/comments")
                .len(),
            0
        );
    }
}

mod check_rerun_interceptor {
    use super::*;

    fn check_run_text() -> String {
        format!(
            "
    #### :x: Validator: TITLE * :x:
      ***title must begins with \"feat,test,chore\"
      *** Input : use-case: title Settings : ```{{\"begins_with\":{{\"match\":[\"feat\",\"test\",\"chore\"]}}}}```
      {}
  ",
            metadata::serialize(&json!({"id": 123, "eventName": "pull_request", "action": "unlabeled"}))
        )
    }

    fn check_run_payload(action: &str) -> Value {
        let mut payload = pull_request_payload("creator");
        payload["action"] = json!(action);
        payload["check_run"] = json!({"output": {"text": check_run_text()}});
        payload
    }

    fn pull_request_456_stub() -> Stub {
        stub(
            "GET",
            "/repos/owner/repoName/pulls/1",
            &json!({"number": 456}),
        )
    }

    #[tokio::test]
    async fn context_changes_only_for_a_rerequested_check_run_with_matching_id_and_pull_requests() {
        let github = MockedGithub::start(vec![pull_request_456_stub()], &[]).await;
        let mut context = github.context("check_run", check_run_payload("created"));
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "check_run");

        context.payload.to_mut()["action"] = json!("rerequested");
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "check_run");

        context.payload.to_mut()["check_run"]["pull_requests"] = json!([{"number": 1}]);
        context.payload.to_mut()["check_run"]["id"] = json!(123);
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "pull_request");
    }

    #[tokio::test]
    async fn rerequested_check_run_becomes_the_event_stored_in_its_metadata() {
        let github = MockedGithub::start(vec![pull_request_456_stub()], &[]).await;
        let mut payload = check_run_payload("rerequested");
        payload["check_run"]["pull_requests"] = json!([{"number": 1}]);
        payload["check_run"]["id"] = json!(123);
        let mut context = github.context("check_run", payload);
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.payload["pull_request"]["number"], 456);
        assert_eq!(context.event_name, "pull_request");
        assert_eq!(context.payload["action"], "unlabeled");
    }
}

mod milestoned_interceptor {
    use super::*;

    #[tokio::test]
    async fn milestoned_pull_request_issue_becomes_a_pull_request_event() {
        let github = MockedGithub::start(
            vec![stub(
                "GET",
                "/repos/owner/repoName/pulls/12",
                &json!({"number": 12}),
            )],
            &[],
        )
        .await;
        let mut payload = pull_request_payload("creator");
        payload["action"] = json!("milestoned");
        payload["issue"] = json!({"number": 12, "pull_request": {}});
        let mut context = github.context("issues", payload);
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.payload["pull_request"]["number"], 12);
        assert_eq!(context.event_name, "pull_request");
    }
}

mod push_interceptor {
    use super::*;

    const RE_VALIDATION_COMMENTS_PATH: &str = "/repos/owner/repoName/issues/7/comments";
    const CONFIG_RE_VALIDATING_PUSHED_PULL_REQUESTS: &str = "
version: 2
mergeable:
  - when: pull_request.push_synchronize
    validate: []
    pass:
      - do: comment
        payload:
          body: re-validated after the configuration changed
";

    async fn github_with_one_open_pull_request() -> MockedGithub {
        let open_pull_request = json!({
            "number": 7,
            "title": "PR1",
            "user": {"login": "creator"},
            "base": {"repo": {"full_name": "owner/test"}, "ref": "main", "sha": "sha2"},
            "head": {"repo": {"full_name": "owner/test"}, "ref": "test", "sha": "sha1"},
        });
        MockedGithub::start(
            vec![
                stub(
                    "GET",
                    "/repos/owner/repoName/pulls",
                    &json!([open_pull_request]),
                ),
                stub("GET", "/repos/owner/repoName/issues/7/comments", &json!([])),
            ],
            &[(
                ".github/mergeable.yml",
                CONFIG_RE_VALIDATING_PUSHED_PULL_REQUESTS,
            )],
        )
        .await
    }

    fn push_payload(head_commit: &Value) -> Value {
        json!({
            "ref": "refs/heads/main",
            "repository": {"name": REPOSITORY_NAME, "full_name": "fullRepoName", "owner": {"login": OWNER}},
            "sender": {"login": "initiator"},
            "head_commit": head_commit,
        })
    }

    fn config_modifying_commit() -> Value {
        json!({"added": [], "modified": [".github/mergeable.yml"]})
    }

    #[tokio::test]
    async fn context_is_not_modified_by_other_events_or_by_a_config_push() {
        let github = github_with_one_open_pull_request().await;
        let mut context = github.context("issues", push_payload(&Value::Null));
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "issues");

        let mut context = github.context("push", push_payload(&config_modifying_commit()));
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "push");
    }

    #[tokio::test]
    async fn config_push_runs_the_workflow_for_each_open_pull_request_as_push_synchronize() {
        let github = github_with_one_open_pull_request().await;
        let mut context = github.context("push", push_payload(&config_modifying_commit()));
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "push");
        let comments = github
            .wait_for_calls_to("POST", RE_VALIDATION_COMMENTS_PATH, 1)
            .await;
        assert_eq!(comments.len(), 1);
        assert_eq!(
            comments[0]["body"]["body"],
            "re-validated after the configuration changed"
        );
    }

    #[tokio::test]
    async fn push_without_head_commit_does_nothing() {
        let github = github_with_one_open_pull_request().await;
        let mut context = github.context("push", push_payload(&Value::Null));
        interceptors::run_all(&mut context).await.unwrap();
        assert_eq!(context.event_name, "push");
        assert_eq!(github.call_count(), 0);
    }
}

mod settings_loading {
    use super::*;

    const SETTINGS_TEXT: &str = "
mergeable:
  config_path: \"\"
  use_config_cache: false
  use_org_as_default_config: false
  use_config_from_pull_request: true
";

    fn parsed_settings() -> Value {
        json!({"mergeable": {
            "config_path": "",
            "use_config_cache": false,
            "use_org_as_default_config": false,
            "use_config_from_pull_request": true,
        }})
    }

    fn settings_cache_key() -> String {
        format!("{OWNER}/{REPOSITORY_NAME}/settings")
    }

    #[tokio::test]
    async fn bad_yml_reports_one_bad_yml_error() {
        let github = MockedGithub::start(
            vec![],
            &[(ORGANIZATION_SETTINGS_FILE, "mergeable: [unclosed")],
        )
        .await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        let loaded = settings::instance_with_context(&context).await;
        assert_eq!(loaded.errors.len(), 1);
        assert!(loaded.has_error(ErrorCode::BadYml));
    }

    #[tokio::test]
    async fn missing_settings_file_yields_the_default_settings() {
        let github = MockedGithub::start(vec![], &[]).await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        let loaded = settings::instance_with_context(&context).await;
        assert!(!loaded.has_errors());
        assert_eq!(loaded.mergeable, parsed_settings()["mergeable"]);
    }

    #[tokio::test]
    async fn settings_are_fetched_from_the_organization_repository_first() {
        let github =
            MockedGithub::start(vec![], &[(ORGANIZATION_SETTINGS_FILE, SETTINGS_TEXT)]).await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        let fetched = settings::fetch_settings_file(&context).await.unwrap();
        assert_eq!(fetched, parsed_settings());
        assert_eq!(github.calls_to("GET", ORGANIZATION_SETTINGS_PATH).len(), 1);
        assert_eq!(github.call_count(), 1);
    }

    #[tokio::test]
    async fn settings_cache_fetches_the_settings_file_only_once() {
        let github = MockedGithub::start_with_environment(
            vec![],
            &[(ORGANIZATION_SETTINGS_FILE, SETTINGS_TEXT)],
            &[("USE_SETTINGS_CACHE", "true")],
        )
        .await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        let settings_cache = &context.app.settings_cache;
        assert_eq!(settings_cache.keys().await.len(), 0);
        assert_eq!(github.call_count(), 0);

        let fetched = settings::fetch_settings_file(&context).await.unwrap();
        assert_eq!(github.call_count(), 1);
        assert_eq!(fetched, parsed_settings());
        assert_eq!(settings_cache.keys().await.len(), 1);
        assert_eq!(
            settings_cache.get(&settings_cache_key()).await,
            Some(parsed_settings())
        );

        let cached = settings::fetch_settings_file(&context).await.unwrap();
        assert_eq!(cached, parsed_settings());
        assert_eq!(github.call_count(), 1);
    }

    #[tokio::test]
    async fn cached_settings_are_used_without_fetching() {
        let github = MockedGithub::start_with_environment(
            vec![],
            &[(ORGANIZATION_SETTINGS_FILE, SETTINGS_TEXT)],
            &[("USE_SETTINGS_CACHE", "true")],
        )
        .await;
        let context = github.context("pull_request", pull_request_payload("creator"));
        context
            .app
            .settings_cache
            .set(&settings_cache_key(), parsed_settings())
            .await;
        let fetched = settings::fetch_settings_file(&context).await.unwrap();
        assert_eq!(github.call_count(), 0);
        assert_eq!(fetched, parsed_settings());
    }

    #[tokio::test]
    async fn push_adding_the_settings_file_invalidates_the_settings_cache() {
        let github = MockedGithub::start_with_environment(
            vec![],
            &[(ORGANIZATION_SETTINGS_FILE, "{}")],
            &[("USE_SETTINGS_CACHE", "true")],
        )
        .await;
        let mut payload = pull_request_payload("creator");
        payload["head_commit"] = json!({"added": [".github/mergeable.settings.yml"]});
        let context = github.context("push", payload);
        let mut stale_settings = parsed_settings();
        stale_settings["version"] = json!(1);
        stale_settings["mergeable"]["config_path"] = json!("stale.yml");
        context
            .app
            .settings_cache
            .set(&settings_cache_key(), stale_settings)
            .await;

        let fetched = settings::fetch_settings_file(&context).await.unwrap();

        let mut expected_settings = parsed_settings();
        expected_settings["version"] = json!(1);
        assert_eq!(github.call_count(), 1);
        assert_eq!(fetched, expected_settings);
        assert_eq!(context.app.settings_cache.keys().await.len(), 1);
    }
}

mod mergeable_start {
    use super::*;

    fn app_config(extra_variables: &[(&str, &str)]) -> AppConfig {
        AppConfig::from_environment(&test_environment(1, extra_variables)).unwrap()
    }

    #[test]
    fn development_mode_schedules_every_two_seconds_and_serves_webhooks() {
        let config = app_config(&[("NODE_ENV", "development"), ("MERGEABLE_SCHEDULER", "true")]);
        let scheduler = config.scheduler.unwrap();
        assert!((scheduler.interval_milliseconds - 2000.0).abs() < f64::EPSILON);
        assert_eq!(config.webhook_path, "/api/github/webhooks");
    }

    #[test]
    fn production_mode_schedules_every_hour_and_serves_webhooks() {
        let config = app_config(&[("NODE_ENV", "production"), ("MERGEABLE_SCHEDULER", "true")]);
        let scheduler = config.scheduler.unwrap();
        assert!((scheduler.interval_milliseconds - 3_600_000.0).abs() < f64::EPSILON);
        assert_eq!(config.webhook_path, "/api/github/webhooks");
    }
}
