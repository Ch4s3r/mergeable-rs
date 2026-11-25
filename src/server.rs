use crate::app_config::AppConfig;
use crate::app_state::AppState;
use crate::context::{Context, SharedPayload};
use crate::error::{Error, Result};
use crate::github::app_auth::AppJwtClient;
use crate::github::client::{GithubApp, GithubAppSettings, InstallationClient};
use crate::github::throttle::RequestThrottle;
use crate::json_value;
use crate::metrics::{self, Metrics};
use crate::scheduler::Scheduler;
use crate::workflow;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures::StreamExt;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Map, Value, json};
use sha2::Sha256;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::{JoinError, JoinHandle};
use tracing::Instrument;

const STILL_PROCESSING_AFTER: Duration = Duration::from_secs(9);
const GRACEFUL_SHUTDOWN_LIMIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum StartupError {
    GithubClient(Error),
    Listen {
        address: String,
        source: std::io::Error,
    },
    Serve(std::io::Error),
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartupError::GithubClient(error) => write!(formatter, "{error}"),
            StartupError::Listen { address, source } => {
                write!(formatter, "cannot listen on {address}: {source}")
            }
            StartupError::Serve(source) => write!(formatter, "server error: {source}"),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StartupError::GithubClient(error) => Some(error),
            StartupError::Listen { source, .. } | StartupError::Serve(source) => Some(source),
        }
    }
}

impl From<Error> for StartupError {
    fn from(error: Error) -> Self {
        StartupError::GithubClient(error)
    }
}

pub struct RunningServer {
    address: SocketAddr,
    shutdown_signal: oneshot::Sender<()>,
    serving: JoinHandle<std::io::Result<()>>,
}

fn serving_outcome(
    joined: std::result::Result<std::io::Result<()>, tokio::task::JoinError>,
) -> std::result::Result<(), StartupError> {
    match joined {
        Ok(served) => served.map_err(StartupError::Serve),
        Err(join_error) => Err(StartupError::Serve(std::io::Error::other(join_error))),
    }
}

impl RunningServer {
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub async fn wait(self) -> std::result::Result<(), StartupError> {
        let RunningServer {
            shutdown_signal,
            serving,
            ..
        } = self;
        let outcome = serving_outcome(serving.await);
        drop(shutdown_signal);
        outcome
    }

    pub async fn shutdown(self) -> std::result::Result<(), StartupError> {
        let RunningServer {
            shutdown_signal,
            mut serving,
            ..
        } = self;
        let _ = shutdown_signal.send(());
        if let Ok(joined) = tokio::time::timeout(GRACEFUL_SHUTDOWN_LIMIT, &mut serving).await {
            return serving_outcome(joined);
        }
        serving.abort();
        Ok(())
    }
}

pub async fn start(config: AppConfig) -> std::result::Result<RunningServer, StartupError> {
    let metrics = config
        .metrics_endpoint_enabled
        .then(|| Arc::new(Metrics::new()));
    let github = Arc::new(GithubApp::new(
        GithubAppSettings {
            base_url: config.github_base_url.clone(),
            app_id: config.app_id.clone(),
            private_key_pem: config.private_key_pem.clone(),
            retry_base_delay: config.github_retry_base_delay,
            throttle: config.github_throttle_enabled.then(RequestThrottle::new),
            http_timeouts: config.outgoing_http_timeouts,
        },
        metrics.clone(),
    )?);
    let app_client = Arc::new(AppJwtClient::new(
        &config.github_base_url,
        &config.app_id,
        &config.private_key_pem,
        config.outgoing_http_timeouts,
    )?);
    let app = Arc::new(AppState::with_metrics(
        config.behavior.clone(),
        metrics.as_ref(),
    ));
    tracing::info!(run_mode = ?config.run_mode, "app.mode");
    let scheduler = if let Some(options) = &config.scheduler {
        tracing::info!(
            interval_milliseconds = options.interval_milliseconds,
            "scheduler.started"
        );
        Some(Scheduler::start(
            Arc::clone(&github),
            Arc::clone(&app),
            Arc::clone(&app_client),
            options.clone(),
        ))
    } else {
        tracing::info!("scheduler.disabled");
        None
    };
    let requested_address = format!("{}:{}", config.host, config.port);
    let state = ServerState {
        github,
        app,
        app_client,
        scheduler,
        webhook_secret: Arc::from(config.webhook_secret.as_str()),
        metrics,
        webhook_slots: Arc::new(Semaphore::new(config.max_concurrent_webhooks)),
    };
    let router = router(&config, state);
    let listener = tokio::net::TcpListener::bind(&requested_address)
        .await
        .map_err(|source| StartupError::Listen {
            address: requested_address.clone(),
            source,
        })?;
    let address = listener
        .local_addr()
        .map_err(|source| StartupError::Listen {
            address: requested_address,
            source,
        })?;
    tracing::info!(%address, "server.listening");
    let (shutdown_signal, shutdown_requested) = oneshot::channel::<()>();
    let serving = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_requested.await;
            })
            .await
    });
    Ok(RunningServer {
        address,
        shutdown_signal,
        serving,
    })
}

#[derive(Clone)]
pub struct ServerState {
    pub github: Arc<GithubApp>,
    pub app: Arc<AppState>,
    pub app_client: Arc<AppJwtClient>,
    pub scheduler: Option<Arc<Scheduler>>,
    pub webhook_secret: Arc<str>,
    pub metrics: Option<Arc<Metrics>>,
    pub webhook_slots: Arc<Semaphore>,
}

pub fn router(config: &AppConfig, state: ServerState) -> Router {
    let mut fall_through_routes: Router<ServerState> = Router::new().fallback(not_found_page);
    if let Some(registry) = state
        .metrics
        .clone()
        .filter(|_| config.metrics_endpoint_enabled)
    {
        fall_through_routes = fall_through_routes
            .route(metrics::METRICS_PATH, get(render_metrics))
            .layer(axum::middleware::from_fn(
                move |request: Request, next: Next| {
                    record_request_metrics(Arc::clone(&registry), request, next)
                },
            ));
    }
    let mut router = Router::new()
        .route(
            &config.webhook_path,
            post(receive_webhook).fallback(unknown_webhook_route),
        )
        .route("/ping", get(|| async { "PONG" }))
        .route("/probot", get(landing_page))
        .route("/", get(redirect_to_landing_page));
    if config.rate_limit_endpoint_enabled {
        router = router.route(
            "/github-ratelimit/{installation_id}",
            get(github_rate_limit),
        );
    }
    router.merge(fall_through_routes).with_state(state)
}

async fn record_request_metrics(registry: Arc<Metrics>, request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let started = Instant::now();
    let response = next.run(request).await;
    registry.record_http_request(
        &method,
        &path,
        response.status().as_u16(),
        started.elapsed().as_secs_f64(),
    );
    response
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn not_found_page(request: Request) -> Response {
    let resource = request
        .uri()
        .path_and_query()
        .map_or("/", axum::http::uri::PathAndQuery::as_str);
    let body = format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Error</title>\n</head>\n<body>\n<pre>Cannot {} {}</pre>\n</body>\n</html>\n",
        request.method(),
        escape_html(resource)
    );
    (
        StatusCode::NOT_FOUND,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

async fn unknown_webhook_route(request: Request) -> Response {
    let url = request
        .uri()
        .path_and_query()
        .map_or("/", axum::http::uri::PathAndQuery::as_str);
    json_error(
        StatusCode::NOT_FOUND,
        &format!("Unknown route: {} {url}", request.method()),
    )
}

async fn redirect_to_landing_page() -> Response {
    (
        StatusCode::FOUND,
        [
            (header::LOCATION, "/probot"),
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
        ],
        "Found. Redirecting to /probot",
    )
        .into_response()
}

async fn landing_page() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!DOCTYPE html><html><head><title>Mergeable</title></head><body><h1>Mergeable is running</h1></body></html>",
    )
}

async fn render_metrics(State(state): State<ServerState>) -> impl IntoResponse {
    let body = state
        .metrics
        .as_ref()
        .map(|registry| registry.render())
        .unwrap_or_default();
    ([(header::CONTENT_TYPE, metrics::CONTENT_TYPE)], body)
}

fn json_response(value: Option<&Value>) -> Response {
    let body = value.map(json_value::stringify).unwrap_or_default();
    (
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        body,
    )
        .into_response()
}

pub async fn rate_limit_core(
    app_client: &AppJwtClient,
    installation_id: &str,
) -> Result<Option<Value>> {
    let mut data = app_client
        .rate_limit_as_installation(installation_id)
        .await?;
    json_value::take_path(&mut data, &["resources", "core"])
}

async fn github_rate_limit(
    State(state): State<ServerState>,
    Path(installation_id): Path<String>,
) -> Response {
    if installation_id.is_empty() {
        return json_response(Some(&json!("missing parameter `installationId`")));
    }
    if !is_installation_id(&installation_id) {
        return json_response(Some(&json!(
            "installation error: `installationId` must be a number"
        )));
    }
    match rate_limit_core(&state.app_client, &installation_id).await {
        Ok(core) => json_response(core.as_ref()),
        Err(error) => json_response(Some(&json!(format!(
            "installation error: {}",
            error.message()
        )))),
    }
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        json_value::stringify(&json!({"error": message})),
    )
        .into_response()
}

#[must_use]
pub fn signature_matches(secret: &str, body: &[u8], signature_header: &str) -> bool {
    let Some(hex_signature) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

fn is_installation_id(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn header_text<'headers>(headers: &'headers HeaderMap, name: &str) -> &'headers str {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

async fn receive_webhook(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = header_text(&headers, "content-type");
    if !content_type.starts_with("application/json") {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            [(header::CONTENT_TYPE, "application/json"), (header::ACCEPT, "application/json")],
            json_value::stringify(&json!({"error": "Unsupported \"Content-Type\" header value. Must be \"application/json\""})),
        )
            .into_response();
    }
    let missing: Vec<&str> = ["x-github-event", "x-hub-signature-256", "x-github-delivery"]
        .into_iter()
        .filter(|name| !headers.contains_key(*name))
        .collect();
    if !missing.is_empty() {
        return json_error(
            StatusCode::BAD_REQUEST,
            &format!("Required headers missing: {}", missing.join(", ")),
        );
    }
    let signature = header_text(&headers, "x-hub-signature-256");
    if !signature_matches(&state.webhook_secret, &body, signature) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "Error: [@octokit/webhooks] signature does not match event payload and secret",
        );
    }
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, &format!("SyntaxError: {error}")),
    };
    let event_name = header_text(&headers, "x-github-event").to_string();
    let delivery_id = header_text(&headers, "x-github-delivery").to_string();
    let webhook_slots = Arc::clone(&state.webhook_slots);
    let overload_log_fields = (delivery_id.clone(), event_name.clone());
    let handler = handle_event(state, delivery_id, event_name, SharedPayload::from(payload));
    match handle_within_answer_window(webhook_slots, STILL_PROCESSING_AFTER, handler).await {
        WebhookHandling::Overloaded => {
            let (delivery_id, event_name) = overload_log_fields;
            tracing::warn!(%delivery_id, %event_name, "webhook.rejected.overloaded");
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Too many webhooks are being processed, try again later",
            )
        }
        WebhookHandling::StillProcessing => {
            (StatusCode::ACCEPTED, "still processing\n").into_response()
        }
        WebhookHandling::Finished(Ok(())) => (StatusCode::OK, "ok\n").into_response(),
        WebhookHandling::Finished(Err(error)) => {
            let status = error
                .status()
                .and_then(|code| StatusCode::from_u16(code).ok())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            json_error(status, &error.to_string())
        }
        WebhookHandling::Crashed(join_error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Error: {join_error}"),
        ),
    }
}

#[derive(Debug)]
enum WebhookHandling {
    Overloaded,
    StillProcessing,
    Finished(Result<()>),
    Crashed(JoinError),
}

async fn handle_within_answer_window(
    webhook_slots: Arc<Semaphore>,
    answer_window: Duration,
    handler: impl Future<Output = Result<()>> + Send + 'static,
) -> WebhookHandling {
    let answer_deadline = tokio::time::Instant::now() + answer_window;
    let Ok(Ok(webhook_slot)) =
        tokio::time::timeout_at(answer_deadline, webhook_slots.acquire_owned()).await
    else {
        return WebhookHandling::Overloaded;
    };
    let handling = tokio::spawn(async move {
        let outcome = handler.await;
        drop(webhook_slot);
        outcome
    });
    match tokio::time::timeout_at(answer_deadline, handling).await {
        Err(_) => WebhookHandling::StillProcessing,
        Ok(Ok(outcome)) => WebhookHandling::Finished(outcome),
        Ok(Err(join_error)) => WebhookHandling::Crashed(join_error),
    }
}

fn repository_full_names(payload: &Value, field: &str) -> Result<Value> {
    let repositories = json_value::get(payload, field);
    json_value::read(repositories, "map")?;
    let Some(Value::Array(items)) = repositories else {
        return Err(Error::type_mismatch(format!(
            "context.payload.{field}.map is not a function"
        )));
    };
    let names = items
        .iter()
        .map(|repository| {
            Ok(json_value::read(Some(repository), "full_name")?
                .cloned()
                .unwrap_or(Value::Null))
        })
        .collect::<Result<Vec<Value>>>()?;
    Ok(Value::Array(names))
}

fn log_event_received(context: &Context) -> Result<()> {
    let event = context.event_label();
    let mut entry = Map::new();
    entry.insert("logType".to_string(), json!("event_received"));
    if !context.event_id.is_empty() {
        entry.insert("eventId".to_string(), json!(context.event_id));
    }
    entry.insert("event".to_string(), json!(event));
    let payload = &context.payload;
    if event.contains("installation") {
        let installation = json_value::get(payload, "installation");
        let (added, removed) = if event == "installation" {
            (repository_full_names(payload, "repositories")?, json!([]))
        } else if event == "installation_repositories" {
            (
                repository_full_names(payload, "repositories_added")?,
                repository_full_names(payload, "repositories_removed")?,
            )
        } else {
            (json!([]), json!([]))
        };
        let installation_id = json_value::read(installation, "id")?.cloned();
        let account = json_value::read(installation, "account")?;
        let account_login = json_value::read(account, "login")?.cloned();
        let account_type = json_value::read(account, "type")?.cloned();
        let sender_login = json_value::read(json_value::get(payload, "sender"), "login")?.cloned();
        let mut insert_defined = |key: &str, value: Option<Value>| {
            if let Some(present) = value {
                entry.insert(key.to_string(), present);
            }
        };
        insert_defined("installationId", installation_id);
        insert_defined("account", account_login);
        insert_defined("accountType", account_type);
        insert_defined(
            "repositories",
            Some(json!({"added": added, "removed": removed})),
        );
        insert_defined("sender", sender_login);
    }
    if let Some(repository) = payload.get("repository") {
        for (key, field) in [
            ("repo", "full_name"),
            ("url", "html_url"),
            ("isPrivate", "private"),
        ] {
            if let Some(value) = json_value::read(Some(repository), field)? {
                entry.insert(key.to_string(), value.clone());
            }
        }
    }
    let details = Value::Object(entry);
    tracing::info!(%details, "event.received");
    Ok(())
}

fn event_client(github: &Arc<GithubApp>, event_name: &str, payload: &Value) -> InstallationClient {
    let installation_id = json_value::get_path(payload, &["installation", "id"])
        .filter(|id| json_value::is_truthy(id));
    let action = payload
        .get("action")
        .filter(|action| json_value::is_truthy(action));
    let full_event_name = match action {
        Some(action) => format!(
            "{event_name}.{}",
            json_value::to_display_string(Some(action))
        ),
        None => event_name.to_string(),
    };
    let access_revoked = event_name == "installation"
        && matches!(
            action
                .map(|action| json_value::to_display_string(Some(action)))
                .as_deref(),
            Some("suspend" | "deleted")
        );
    match installation_id {
        None => github.unauthenticated(format!(
            "Handling a \"{full_event_name}\" event: an \"installation\" key is missing. The installation ID cannot be determined"
        )),
        Some(id) if access_revoked => github.unauthenticated(format!(
            "Handling a \"{full_event_name}\" event: The app's access has been revoked from @octokit (id: {})",
            json_value::to_display_string(Some(id))
        )),
        Some(id) => github.installation(id.as_u64()),
    }
}

pub async fn run_mergeable(
    github: Arc<GithubApp>,
    app: Arc<AppState>,
    delivery_id: String,
    event_name: String,
    payload: SharedPayload,
) -> Result<()> {
    let client = event_client(&github, &event_name, &payload);
    let context = Context::new(delivery_id, event_name, payload, client, app);
    let span = tracing::info_span!(
        "event",
        event_id = %context.event_id,
        event = %context.event_label(),
        repository = %context.repository_full_name(),
    );
    span.in_scope(|| log_event_received(&context))?;
    workflow::execute(context).instrument(span).await
}

pub async fn handle_event(
    state: ServerState,
    delivery_id: String,
    event_name: String,
    payload: SharedPayload,
) -> Result<()> {
    let mut handlers: FuturesUnordered<BoxFuture<'static, Result<()>>> = FuturesUnordered::new();
    if let Some(scheduler) = state.scheduler.clone()
        && Scheduler::listens_to(&event_name, &payload)
    {
        let hook_event_name = event_name.clone();
        let hook_payload = payload.clone();
        handlers.push(Box::pin(async move {
            scheduler
                .handle_webhook(&hook_event_name, &hook_payload)
                .await
        }));
    }
    handlers.push(Box::pin(run_mergeable(
        state.github,
        state.app,
        delivery_id,
        event_name,
        payload,
    )));
    let mut first_error = None;
    while let Some(outcome) = handlers.next().await {
        if let Err(error) = outcome
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = b"Hello, World!";

    fn random_secret() -> String {
        hex::encode(rand::random::<[u8; 32]>())
    }

    fn hex_digest<Signature: Mac + KeyInit>(secret: &str, body: &[u8]) -> String {
        let mut signer = <Signature as KeyInit>::new_from_slice(secret.as_bytes()).unwrap();
        signer.update(body);
        hex::encode(signer.finalize().into_bytes())
    }

    fn sha256_signature(secret: &str, body: &[u8]) -> String {
        format!("sha256={}", hex_digest::<Hmac<Sha256>>(secret, body))
    }

    #[test]
    fn webhook_signed_with_the_secret_using_sha256_is_accepted() {
        let secret = random_secret();
        assert!(signature_matches(
            &secret,
            BODY,
            &sha256_signature(&secret, BODY)
        ));
    }

    #[test]
    fn webhook_signed_with_another_secret_or_body_is_rejected() {
        let secret = random_secret();
        let signature = sha256_signature(&secret, BODY);
        assert!(!signature_matches(&random_secret(), BODY, &signature));
        assert!(!signature_matches(&secret, b"Hello, World?", &signature));
        assert!(!signature_matches(&secret, BODY, ""));
        assert!(!signature_matches(&secret, BODY, "sha256=not-hex"));
    }

    #[test]
    fn sha1_signatures_are_rejected_even_when_they_match() {
        let secret = random_secret();
        let sha1 = hex_digest::<Hmac<sha1::Sha1>>(&secret, BODY);
        assert!(!signature_matches(&secret, BODY, &format!("sha1={sha1}")));
        assert!(!signature_matches(&secret, BODY, &sha1));
    }

    #[test]
    fn rate_limit_route_accepts_only_numeric_installation_ids() {
        assert!(is_installation_id("12345"));
        assert!(!is_installation_id(""));
        assert!(!is_installation_id(".."));
        assert!(!is_installation_id("1/../../app"));
        assert!(!is_installation_id("-1"));
    }
    fn webhook_slots(available: usize) -> Arc<Semaphore> {
        Arc::new(Semaphore::new(available))
    }

    #[tokio::test]
    async fn webhook_beyond_the_limit_is_rejected_when_no_slot_frees_up_within_the_answer_window() {
        let slots = webhook_slots(1);
        let _busy_slot = Arc::clone(&slots).acquire_owned().await;
        let handling =
            handle_within_answer_window(slots, Duration::from_millis(50), async { Ok(()) }).await;
        assert!(matches!(handling, WebhookHandling::Overloaded));
    }

    #[tokio::test]
    async fn webhook_beyond_the_limit_waits_for_a_slot_that_frees_up_in_time() {
        let slots = webhook_slots(1);
        let busy_slot = Arc::clone(&slots).acquire_owned().await;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            drop(busy_slot);
        });
        let handling =
            handle_within_answer_window(slots, Duration::from_secs(5), async { Ok(()) }).await;
        assert!(matches!(handling, WebhookHandling::Finished(Ok(()))));
    }

    #[tokio::test]
    async fn webhook_still_processing_after_the_answer_keeps_its_slot_until_it_finishes() {
        let slots = webhook_slots(1);
        let (finish, finished) = oneshot::channel::<()>();
        let handling = handle_within_answer_window(
            Arc::clone(&slots),
            Duration::from_millis(30),
            async move {
                let _ = finished.await;
                Ok(())
            },
        )
        .await;
        assert!(matches!(handling, WebhookHandling::StillProcessing));
        assert_eq!(slots.available_permits(), 0);
        let _ = finish.send(());
        let slot_returned = tokio::time::timeout(Duration::from_secs(5), slots.acquire()).await;
        assert!(slot_returned.is_ok_and(|slot| slot.is_ok()));
    }
}
