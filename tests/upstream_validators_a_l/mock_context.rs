use crate::contract::case::Stub;
use crate::contract::mock::{MockServer, Scenario};
use base64::Engine;
use mergeable::app_state::AppState;
use mergeable::context::Context;
use mergeable::github::client::{GithubApp, GithubAppSettings};
use mergeable::registry::Registry;
use mergeable::validators;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rsa::PrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const REPOSITORY_PATH: &str = "/repos/owner/repoName";
const UNUSED_SIGNING_KEY_SEED: u64 = 0x7661_6c69_6461_746f;

static UNUSED_SIGNING_KEY_PEM: LazyLock<String> = LazyLock::new(|| {
    let mut seeded_rng = StdRng::seed_from_u64(UNUSED_SIGNING_KEY_SEED);
    let key = PrivateKey::new(&mut seeded_rng, 1024).expect("test signing key");
    key.to_pkcs1_pem(LineEnding::LF)
        .expect("test signing key pem")
        .to_string()
});

pub struct MockedGithub {
    server: MockServer,
    pub context: Context,
}

impl MockedGithub {
    pub async fn process_validate(&self, validator_key: &str, settings: &Value) -> Value {
        let validator = validators::create(validator_key).expect("known validator");
        validators::process_validate(&*validator, &self.context, settings, &Registry::default())
            .await
            .expect("validation result")
    }

    pub fn payload_mut(&mut self) -> &mut Value {
        self.context.payload.to_mut()
    }

    pub fn requested_queries(&self) -> Vec<Value> {
        self.server
            .github
            .recorded_calls()
            .iter()
            .map(|call| call["query"].clone())
            .collect()
    }

    pub fn requested_paths(&self) -> Vec<String> {
        self.server
            .github
            .recorded_calls()
            .iter()
            .map(|call| call["path"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
}

pub async fn mock_context(options: &Value) -> MockedGithub {
    mock_context_with(options, Vec::new()).await
}

pub async fn mock_context_with(options: &Value, extra_stubs: Vec<Value>) -> MockedGithub {
    let server = MockServer::start().await.expect("mock GitHub API");
    let mut stubs = extra_stubs;
    stubs.extend(stubs_for(options));
    server.github.load_scenario(Scenario {
        home_repository: "fullRepoName".to_owned(),
        stubs: stubs
            .into_iter()
            .map(|stub| serde_json::from_value::<Stub>(stub).expect("stub"))
            .collect(),
        files: BTreeMap::new(),
    });
    let app = Arc::new(
        GithubApp::new(
            GithubAppSettings {
                base_url: format!("http://127.0.0.1:{}/api/v3", server.port),
                app_id: "1".to_owned(),
                private_key_pem: UNUSED_SIGNING_KEY_PEM.clone(),
                retry_base_delay: Duration::from_millis(1),
                throttle: None,
            },
            None,
        )
        .expect("GitHub app"),
    );
    let event_name = options
        .get("eventName")
        .and_then(Value::as_str)
        .unwrap_or("pull_request");
    let mut context = Context::new(
        "event-id".to_owned(),
        event_name.to_owned(),
        payload(options),
        app.installation(None),
        Arc::new(AppState::default()),
    );
    context.global_settings = Some(Arc::new(json!({})));
    MockedGithub { server, context }
}

fn option_or(options: &Value, key: &str, default: Value) -> Value {
    options
        .get(key)
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or(default)
}

fn insert_when_given(target: &mut Value, options: &Value, option_key: &str, field: &str) {
    if let (Some(value), Value::Object(fields)) = (options.get(option_key), target) {
        fields.insert(field.to_owned(), value.clone());
    }
}

fn now_iso() -> Value {
    json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

pub fn payload(options: &Value) -> Value {
    let number = option_or(options, "number", json!(1));
    let title = option_or(options, "title", json!("title"));
    let milestone = option_or(options, "milestone", Value::Null);
    let created_at = option_or(options, "createdAt", now_iso());
    let updated_at = option_or(options, "updatedAt", now_iso());
    let assignees = option_or(options, "assignees", json!([]));
    let mut pull_request = json!({
        "user": {"login": option_or(options, "author", json!("creator"))},
        "title": title,
        "number": number,
        "created_at": created_at,
        "updated_at": updated_at,
        "milestone": milestone,
        "requested_reviewers": option_or(options, "requestedReviewers", json!([])),
        "requested_teams": option_or(options, "requestedTeams", json!([])),
        "base": {
            "repo": {"full_name": "owner/test", "private": false},
            "ref": option_or(options, "baseRef", json!("baseRef")),
            "sha": "sha2",
        },
        "head": {
            "ref": option_or(options, "headRef", json!("test")),
            "sha": "sha1",
            "repo": {"full_name": "owner/test", "issues_url": "testRepo/issues{/number}"},
        },
        "assignees": assignees,
    });
    insert_when_given(&mut pull_request, options, "body", "body");
    let mut issue = json!({
        "user": {"login": "creator"},
        "title": title,
        "number": number,
        "milestone": milestone,
        "created_at": created_at,
        "updated_at": updated_at,
        "assignees": assignees,
        "pull_request": {},
    });
    insert_when_given(&mut issue, options, "body", "body");
    let mut payload = json!({
        "sha": "sha1",
        "action": "opened",
        "repository": {"name": "repoName", "full_name": "fullRepoName", "owner": {"login": "owner"}},
        "sender": {"login": "initiator"},
        "check_suite": {"pull_requests": [{"number": 1}]},
        "pull_request": pull_request,
        "issue": issue,
    });
    insert_when_given(&mut payload, options, "issueComment", "comment");
    payload
}

fn changed_file(file: &Value) -> Value {
    match file {
        Value::String(filename) => json!({
            "filename": filename,
            "status": "modified",
            "additions": 0,
            "deletions": 0,
            "changes": 0,
        }),
        other => other.clone(),
    }
}

pub fn stub(path: &str, body: &Value) -> Value {
    json!({"method": "GET", "path": path, "status": 200, "body": body})
}

pub fn repository_stub(path_in_repository: &str, body: &Value) -> Value {
    stub(&format!("{REPOSITORY_PATH}{path_in_repository}"), body)
}

pub fn file_content_stub(file_path: &str, content: &str) -> Value {
    repository_stub(
        &format!("/contents/{file_path}"),
        &json!({"content": base64::engine::general_purpose::STANDARD.encode(content)}),
    )
}

pub fn team_members_stub(organization: &str, team_slug: &str, logins: &[&str]) -> Value {
    let members: Vec<Value> = logins.iter().map(|login| json!({"login": login})).collect();
    stub(
        &format!("/orgs/{organization}/teams/{team_slug}/members"),
        &Value::Array(members),
    )
}

pub fn active_team_membership_stub(organization: &str, team_slug: &str, login: &str) -> Value {
    stub(
        &format!("/orgs/{organization}/teams/{team_slug}/memberships/{login}"),
        &json!({"state": "active"}),
    )
}

fn stubs_for(options: &Value) -> Vec<Value> {
    let number = option_or(options, "number", json!(1));
    let listed = |key: &str| option_or(options, key, json!([]));
    let files: Vec<Value> = listed("files")
        .as_array()
        .map(|files| files.iter().map(changed_file).collect())
        .unwrap_or_default();
    let mut stubs = vec![
        repository_stub(&format!("/pulls/{number}/files"), &Value::Array(files)),
        repository_stub(&format!("/pulls/{number}/commits"), &listed("commits")),
        repository_stub(&format!("/pulls/{number}/reviews"), &listed("reviews")),
        repository_stub(&format!("/issues/{number}/labels"), &listed("labels")),
        repository_stub(&format!("/issues/{number}/comments"), &listed("comments")),
        repository_stub("/collaborators", &listed("collaborators")),
    ];
    if let Some(codeowners) = options.get("codeowners").and_then(Value::as_str) {
        stubs.push(file_content_stub(".github/CODEOWNERS", codeowners));
    }
    let mut compare = Map::new();
    if let Some(compared_files) = options.get("compareCommits") {
        compare.insert("files".to_owned(), compared_files.clone());
    }
    stubs.push(repository_stub(
        "/compare/sha2...sha1",
        &Value::Object(compare),
    ));
    stubs
}
