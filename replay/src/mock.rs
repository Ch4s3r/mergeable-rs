use crate::case::{CaseDefinition, Stub};
use crate::normalize::RecordedValueNormalizer;
use anyhow::Result;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::header::{ACCEPT, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use percent_encoding::percent_decode_str;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::task::JoinHandle;

const API_PREFIX: &str = "/api/v3";
const INSTALLATION_PATH_PREFIX: &str = "/app/installations";

#[derive(Debug, Clone)]
pub struct Scenario {
    pub home_repository: String,
    pub stubs: Vec<Stub>,
    pub files: BTreeMap<String, String>,
}

impl Scenario {
    pub fn from_case(definition: &CaseDefinition) -> Scenario {
        Scenario {
            home_repository: definition.repository_full_name(),
            stubs: definition.github.clone(),
            files: definition.files.clone(),
        }
    }

    fn matching_stub(&self, method: &str, path: &str, query: &Map<String, Value>) -> Option<&Stub> {
        self.stubs.iter().find(|stub| {
            stub.method.eq_ignore_ascii_case(method)
                && percent_decode_str(&stub.path).decode_utf8_lossy() == path
                && stub.query.iter().all(|(key, expected_value)| {
                    query.get(key).and_then(Value::as_str)
                        == Some(&value_as_query_text(expected_value))
                })
        })
    }

    fn file_content(&self, repository: &str, file_path: &str) -> Option<&String> {
        let qualified_key = format!("{repository}:{file_path}");
        if repository == self.home_repository {
            self.files
                .get(file_path)
                .or_else(|| self.files.get(&qualified_key))
        } else {
            self.files.get(&qualified_key)
        }
    }
}

fn value_as_query_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

struct MockState {
    scenarios: Vec<Scenario>,
    recording: bool,
    calls: Vec<Value>,
}

pub struct MockGithub {
    state: Mutex<MockState>,
    last_request_at: Mutex<Instant>,
    requests_served: AtomicU64,
    normalizer: RecordedValueNormalizer,
}

struct MockReply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl MockReply {
    fn json(status: u16, body: &Value) -> MockReply {
        MockReply {
            status,
            headers: vec![(
                CONTENT_TYPE.to_string(),
                "application/json; charset=utf-8".to_owned(),
            )],
            body: body.to_string().into_bytes(),
        }
    }

    fn not_found() -> MockReply {
        MockReply::json(
            404,
            &json!({"message": "Not Found", "documentation_url": "https://docs.github.com/rest"}),
        )
    }
}

impl IntoResponse for MockReply {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit", HeaderValue::from_static("5000"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("4999"));
        headers.insert("x-ratelimit-reset", HeaderValue::from_static("4102444800"));
        for (name, value) in &self.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(name, value);
            }
        }
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, headers, Body::from(self.body)).into_response()
    }
}

impl MockGithub {
    pub fn new(mock_origin: &str) -> Result<MockGithub> {
        Ok(MockGithub {
            state: Mutex::new(MockState {
                scenarios: Vec::new(),
                recording: false,
                calls: Vec::new(),
            }),
            last_request_at: Mutex::new(Instant::now()),
            requests_served: AtomicU64::new(0),
            normalizer: RecordedValueNormalizer::new(mock_origin)?,
        })
    }

    fn locked_state(&self) -> MutexGuard<'_, MockState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn load_scenarios(&self, scenarios: Vec<Scenario>, recording: bool) {
        let mut state = self.locked_state();
        state.scenarios = scenarios;
        state.recording = recording;
        state.calls.clear();
    }

    pub fn take_calls(&self) -> Vec<Value> {
        std::mem::take(&mut self.locked_state().calls)
    }

    pub fn recorded_call_count(&self) -> usize {
        self.locked_state().calls.len()
    }

    pub fn last_request_at(&self) -> Instant {
        *self
            .last_request_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn requests_served(&self) -> u64 {
        self.requests_served.load(Ordering::Relaxed)
    }

    fn note_request(&self) {
        self.requests_served.fetch_add(1, Ordering::Relaxed);
        *self
            .last_request_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    }

    fn recorded_body(&self, body: &[u8]) -> Value {
        if body.is_empty() {
            return Value::Null;
        }
        let parsed = serde_json::from_slice(body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).into_owned()));
        self.normalizer.normalize(parsed)
    }

    fn respond(
        &self,
        method: &str,
        path: &str,
        query: &Map<String, Value>,
        accept: &str,
        body: &[u8],
    ) -> MockReply {
        let mut state = self.locked_state();
        if state.recording {
            let call = json!({
                "method": method,
                "path": path,
                "query": self.normalizer.normalize(Value::Object(query.clone())),
                "body": self.recorded_body(body),
            });
            state.calls.push(call);
        }
        let repository = repository_of_path(path);
        let mut candidates: Vec<&Scenario> = state.scenarios.iter().collect();
        if let Some(repository) = &repository {
            candidates.sort_by_key(|scenario| &scenario.home_repository != repository);
        }
        if let Some(stub) = candidates
            .iter()
            .find_map(|scenario| scenario.matching_stub(method, path, query))
        {
            return stub_reply(stub);
        }
        if method == "GET"
            && let Some((repository, file_path)) = contents_request(path)
        {
            // ponytail: in bench mode a file of a shared repository (e.g. owner/.github) comes from the first case that lists it
            if let Some(content) = candidates
                .iter()
                .find_map(|scenario| scenario.file_content(&repository, &file_path))
            {
                return file_reply(&file_path, content, accept);
            }
        }
        MockReply::not_found()
    }
}

fn stub_reply(stub: &Stub) -> MockReply {
    let mut reply = if stub.body.is_null() {
        MockReply {
            status: stub.status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    } else {
        MockReply::json(stub.status, &stub.body)
    };
    reply.headers.extend(
        stub.headers
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    reply
}

fn file_reply(file_path: &str, content: &str, accept: &str) -> MockReply {
    if accept.contains(".raw") {
        let content_type = if std::path::Path::new(file_path)
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            "application/json"
        } else {
            "text/plain; charset=utf-8"
        };
        return MockReply {
            status: 200,
            headers: vec![(CONTENT_TYPE.to_string(), content_type.to_owned())],
            body: content.as_bytes().to_vec(),
        };
    }
    let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
    MockReply::json(
        200,
        &json!({
            "type": "file",
            "encoding": "base64",
            "size": content.len(),
            "name": file_name,
            "path": file_path,
            "content": base64::engine::general_purpose::STANDARD.encode(content),
            "sha": "0000",
        }),
    )
}

fn repository_of_path(path: &str) -> Option<String> {
    let mut segments = path.strip_prefix("/repos/")?.splitn(3, '/');
    let owner = segments.next()?;
    let repository = segments.next()?;
    Some(format!("{owner}/{repository}"))
}

fn contents_request(path: &str) -> Option<(String, String)> {
    let mut segments = path.strip_prefix("/repos/")?.splitn(4, '/');
    let owner = segments.next()?;
    let repository = segments.next()?;
    if segments.next()? != "contents" {
        return None;
    }
    let file_path = segments.next()?;
    Some((format!("{owner}/{repository}"), file_path.to_owned()))
}

fn installation_reply(method: &str, path: &str) -> MockReply {
    let installation_id: u64 = path
        .trim_start_matches(INSTALLATION_PATH_PREFIX)
        .trim_start_matches('/')
        .split('/')
        .next()
        .and_then(|segment| segment.parse().ok())
        .unwrap_or(1);
    if method == "POST" && path.ends_with("/access_tokens") {
        return MockReply::json(
            201,
            &json!({
                "token": "ghs_test",
                "expires_at": "2099-01-01T00:00:00Z",
                "permissions": {},
                "repository_selection": "all",
            }),
        );
    }
    MockReply::json(
        200,
        &json!({
            "id": installation_id,
            "app_id": 1,
            "account": {"login": "replay", "type": "Organization"},
            "permissions": {},
            "events": [],
        }),
    )
}

fn parse_query(raw_query: Option<&str>) -> Map<String, Value> {
    let sorted: BTreeMap<String, String> = raw_query
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect()
        })
        .unwrap_or_default();
    sorted
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect()
}

async fn handle_request(
    State(github): State<Arc<MockGithub>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> MockReply {
    github.note_request();
    let Some(api_path) = uri.path().strip_prefix(API_PREFIX) else {
        return MockReply::not_found();
    };
    let path = percent_decode_str(api_path)
        .decode_utf8_lossy()
        .into_owned();
    if path.starts_with(INSTALLATION_PATH_PREFIX) {
        return installation_reply(method.as_str(), &path);
    }
    let accept = headers
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    github.respond(
        method.as_str(),
        &path,
        &parse_query(uri.query()),
        accept,
        &body,
    )
}

pub struct MockServer {
    pub port: u16,
    pub github: Arc<MockGithub>,
    server_task: JoinHandle<()>,
}

impl MockServer {
    pub async fn start() -> Result<MockServer> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let github = Arc::new(MockGithub::new(&format!("http://127.0.0.1:{port}"))?);
        let router = Router::new()
            .fallback(handle_request)
            .with_state(github.clone());
        let server_task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router).await {
                eprintln!("mock GitHub stopped: {error}");
            }
        });
        Ok(MockServer {
            port,
            github,
            server_task,
        })
    }

    pub fn stop(self) {
        self.server_task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario_with_files() -> Scenario {
        Scenario {
            home_repository: "acme/app".to_owned(),
            stubs: Vec::new(),
            files: BTreeMap::from([
                (".github/mergeable.yml".to_owned(), "version: 2".to_owned()),
                (
                    "acme/.github:.github/mergeable.settings.yml".to_owned(),
                    "mergeable: {}".to_owned(),
                ),
            ]),
        }
    }

    #[test]
    fn files_of_the_payload_repository_need_no_repository_prefix() {
        let scenario = scenario_with_files();
        assert_eq!(
            scenario
                .file_content("acme/app", ".github/mergeable.yml")
                .map(String::as_str),
            Some("version: 2")
        );
        assert_eq!(
            scenario.file_content("acme/.github", ".github/mergeable.yml"),
            None
        );
        assert!(
            scenario
                .file_content("acme/.github", ".github/mergeable.settings.yml")
                .is_some()
        );
    }

    #[test]
    fn contents_path_splits_into_repository_and_file_path() {
        assert_eq!(
            contents_request("/repos/acme/app/contents/.github/mergeable.yml"),
            Some(("acme/app".to_owned(), ".github/mergeable.yml".to_owned()))
        );
    }

    #[test]
    fn stub_query_matches_as_subset() {
        let stub: Stub = serde_json::from_value(json!({
            "method": "GET", "path": "/repos/acme/app/pulls/1/files", "query": {"page": 2}, "body": []
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let scenario = Scenario {
            home_repository: "acme/app".to_owned(),
            stubs: vec![stub],
            files: BTreeMap::new(),
        };
        let query = parse_query(Some("per_page=100&page=2"));
        assert!(
            scenario
                .matching_stub("get", "/repos/acme/app/pulls/1/files", &query)
                .is_some()
        );
        assert!(
            scenario
                .matching_stub("GET", "/repos/acme/app/pulls/1/files", &parse_query(None))
                .is_none()
        );
    }
}
