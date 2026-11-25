mod milestone;
mod project;
mod size;
mod stale;

use crate::contract::case::Stub;
use crate::contract::mock::{MockServer, Scenario};
use mergeable::app_state::AppState;
use mergeable::context::Context;
use mergeable::github::client::{GithubApp, GithubAppSettings};
use mergeable::registry::Registry;
use mergeable::validators::{self, Validator};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rsa::PrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const SIGNING_KEY_SEED: u64 = 0x7661_6c69_6461_746f;

static SIGNING_KEY_PEM: LazyLock<String> = LazyLock::new(|| {
    let mut seeded_rng = StdRng::seed_from_u64(SIGNING_KEY_SEED);
    let key = PrivateKey::new(&mut seeded_rng, 2048).expect("test signing key");
    key.to_pkcs1_pem(LineEnding::LF)
        .expect("test signing key as PEM")
        .to_string()
});

#[derive(Default)]
struct PullRequestFields {
    number: Option<u64>,
    body: Option<&'static str>,
    diff_url: Option<String>,
}

fn pull_request_payload(fields: PullRequestFields) -> Value {
    let mut pull_request = Map::new();
    pull_request.insert("user".to_owned(), json!({"login": "creator"}));
    pull_request.insert("title".to_owned(), json!("title"));
    if let Some(body) = fields.body {
        pull_request.insert("body".to_owned(), json!(body));
    }
    pull_request.insert("number".to_owned(), json!(fields.number.unwrap_or(1)));
    pull_request.insert("milestone".to_owned(), Value::Null);
    pull_request.insert(
        "base".to_owned(),
        json!({"repo": {"full_name": "owner/test", "private": false}, "ref": "baseRef", "sha": "sha2"}),
    );
    pull_request.insert(
        "head".to_owned(),
        json!({"ref": "test", "sha": "sha1", "repo": {"full_name": "owner/test", "issues_url": "testRepo/issues{/number}"}}),
    );
    if let Some(diff_url) = fields.diff_url {
        pull_request.insert("diff_url".to_owned(), json!(diff_url));
    }
    json!({
        "action": "opened",
        "repository": {"name": "repo", "full_name": "owner/repo", "owner": {"login": "owner"}},
        "sender": {"login": "initiator"},
        "pull_request": pull_request,
    })
}

fn get_stub(path: &str, body: Value) -> Stub {
    let mut definition = Map::new();
    definition.insert("method".to_owned(), json!("GET"));
    definition.insert("path".to_owned(), json!(path));
    definition.insert("body".to_owned(), body);
    serde_json::from_value(Value::Object(definition)).expect("stub definition")
}

struct MockedGithub {
    server: MockServer,
    context: Context,
}

impl MockedGithub {
    async fn start(stubs: Vec<Stub>, payload: Value) -> MockedGithub {
        let server = MockServer::start().await.expect("mock GitHub API");
        server.github.load_scenario(Scenario {
            home_repository: "owner/repo".to_owned(),
            stubs,
            files: BTreeMap::new(),
        });
        let app = Arc::new(
            GithubApp::new(
                GithubAppSettings {
                    base_url: format!("http://127.0.0.1:{}/api/v3", server.port),
                    app_id: "1".to_owned(),
                    private_key_pem: SIGNING_KEY_PEM.clone(),
                    retry_base_delay: Duration::from_millis(1),
                    throttle: None,
                    http_timeouts: mergeable::app_config::OutgoingHttpTimeouts::default(),
                },
                None,
            )
            .expect("GitHub App client"),
        );
        let mut context = Context::new(
            "event-id".to_owned(),
            "pull_request".to_owned(),
            payload,
            app.installation(Some(1)),
            Arc::new(AppState::default()),
        );
        context.global_settings = Some(Arc::new(json!({})));
        MockedGithub { server, context }
    }

    async fn process_validate(&self, validator: &dyn Validator, settings: &Value) -> Value {
        validators::process_validate(validator, &self.context, settings, &Registry::default())
            .await
            .expect("validation result")
    }

    async fn validate(&self, validator: &dyn Validator, settings: &Value) -> Value {
        validator
            .validate(&self.context, settings, &Registry::default())
            .await
            .expect("validation result")
    }

    fn recorded_calls(&self) -> Vec<Value> {
        self.server.github.recorded_calls()
    }
}
