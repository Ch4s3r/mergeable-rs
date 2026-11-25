use crate::app_state::AppState;
use crate::context::Context;
use crate::github::client::{GithubApp, GithubAppSettings};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::header::{CONTENT_TYPE, LINK};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rsa::PrivateKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
use serde_json::{Value, json};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Duration;
use tokio::task::JoinHandle;

const STUB_SIGNING_KEY_SEED: u64 = 0x6769_7468_7562_7374;
const STUB_SIGNING_KEY_BITS: usize = 1024;

static STUB_SIGNING_KEY_PEM: LazyLock<String> = LazyLock::new(|| {
    let mut seeded_rng = StdRng::seed_from_u64(STUB_SIGNING_KEY_SEED);
    PrivateKey::new(&mut seeded_rng, STUB_SIGNING_KEY_BITS)
        .and_then(|key| key.to_pkcs1_pem(LineEnding::LF).map_err(Into::into))
        .map_or_else(
            |error| panic!("stub signing key: {error}"),
            |pem| pem.to_string(),
        )
});

#[derive(Clone)]
pub struct StubbedRoute {
    method: Method,
    path: String,
    query: Option<String>,
    status: u16,
    body: Option<Value>,
    next_page: Option<String>,
}

#[must_use]
pub fn route(method: &str, path: &str, status: u16, body: Value) -> StubbedRoute {
    StubbedRoute {
        method: Method::from_bytes(method.as_bytes())
            .unwrap_or_else(|error| panic!("stub method {method}: {error}")),
        path: path.to_string(),
        query: None,
        status,
        body: Some(body),
        next_page: None,
    }
}

#[must_use]
pub fn empty_route(method: &str, path: &str, status: u16) -> StubbedRoute {
    StubbedRoute {
        body: None,
        ..route(method, path, status, Value::Null)
    }
}

#[must_use]
pub fn payment_required(method: &str, path: &str) -> StubbedRoute {
    route(method, path, 402, json!({"message": "Payment Required"}))
}

#[must_use]
pub fn not_found(method: &str, path: &str) -> StubbedRoute {
    route(method, path, 404, json!({"message": "Not Found"}))
}

impl StubbedRoute {
    #[must_use]
    pub fn only_for_query(self, query: &str) -> StubbedRoute {
        StubbedRoute {
            query: Some(query.to_string()),
            ..self
        }
    }

    #[must_use]
    pub fn linking_next_page(self, path_and_query: &str) -> StubbedRoute {
        StubbedRoute {
            next_page: Some(path_and_query.to_string()),
            ..self
        }
    }

    fn matches(&self, method: &Method, uri: &Uri) -> bool {
        self.method == method
            && self.path == uri.path()
            && self
                .query
                .as_deref()
                .is_none_or(|query| uri.query() == Some(query))
    }

    fn response(&self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = match &self.body {
            Some(body) => (
                status,
                [(CONTENT_TYPE, "application/json; charset=utf-8")],
                body.to_string(),
            )
                .into_response(),
            None => (status, Body::empty()).into_response(),
        };
        if let Some(next_page) = &self.next_page
            && let Ok(link) = format!("<{next_page}>; rel=\"next\"").parse()
        {
            response.headers_mut().insert(LINK, link);
        }
        response
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordedRequest {
    pub method: String,
    pub path_and_query: String,
    pub body: Value,
}

struct StubState {
    routes: Vec<StubbedRoute>,
    requests: Mutex<Vec<RecordedRequest>>,
}

async fn answer(
    State(state): State<Arc<StubState>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    state
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(RecordedRequest {
            method: method.to_string(),
            path_and_query: uri
                .path_and_query()
                .map(ToString::to_string)
                .unwrap_or_default(),
            body: serde_json::from_slice(&body).unwrap_or(Value::Null),
        });
    state
        .routes
        .iter()
        .find(|stubbed| stubbed.matches(&method, &uri))
        .map_or_else(
            || not_found(method.as_str(), uri.path()).response(),
            StubbedRoute::response,
        )
}

pub struct StubGithub {
    base_url: String,
    state: Arc<StubState>,
    server_task: JoinHandle<()>,
}

impl StubGithub {
    pub async fn serving(stubbed_routes: Vec<StubbedRoute>) -> StubGithub {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("binding the stub GitHub API: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("stub GitHub API address: {error}"));
        let state = Arc::new(StubState {
            routes: stubbed_routes,
            requests: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .fallback(answer)
            .with_state(Arc::clone(&state));
        let server_task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                panic!("stub GitHub API stopped: {error}");
            }
        });
        StubGithub {
            base_url: format!("http://{address}"),
            state,
            server_task,
        }
    }

    #[must_use]
    pub fn pull_request_context(&self) -> Context {
        let settings = GithubAppSettings {
            base_url: self.base_url.clone(),
            app_id: "1".to_string(),
            private_key_pem: STUB_SIGNING_KEY_PEM.clone(),
            retry_base_delay: Duration::from_millis(1),
            throttle: None,
            http_timeouts: crate::app_config::OutgoingHttpTimeouts::default(),
        };
        let app = Arc::new(
            GithubApp::new(settings, None).unwrap_or_else(|error| panic!("stub app: {error}")),
        );
        Context::new(
            "event-id".to_string(),
            "pull_request".to_string(),
            json!({
                "action": "opened",
                "repository": {"name": "repo", "full_name": "owner/repo", "owner": {"login": "owner"}},
                "pull_request": {"number": 1},
            }),
            app.installation(None),
            Arc::new(AppState::default()),
        )
    }

    #[must_use]
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for StubGithub {
    fn drop(&mut self) {
        self.server_task.abort();
    }
}
