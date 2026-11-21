use mergeable::app_config::AppConfig;
use mergeable::github::app_auth::AppJwtClient;
use mergeable::github::client::{GithubApp, GithubAppSettings};
use mergeable::github::throttle::RequestThrottle;
use mergeable::logging;
use mergeable::metrics::Metrics;
use mergeable::scheduler::{Scheduler, SchedulerOptions};
use mergeable::server::{self, ServerState};
use std::sync::Arc;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let config = match AppConfig::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("mergeable: {message}");
            return std::process::ExitCode::FAILURE;
        }
    };
    logging::init(&config.log_level, config.log_json);
    let metrics = config
        .metrics_endpoint_enabled
        .then(|| Arc::new(Metrics::new()));
    let github = match GithubApp::new(
        GithubAppSettings {
            base_url: config.github_base_url.clone(),
            app_id: config.app_id.clone(),
            private_key_pem: config.private_key_pem.clone(),
            retry_base_delay: config.github_retry_base_delay,
            throttle: RequestThrottle::like_probot(config.github_throttle_enabled),
        },
        metrics.clone(),
    ) {
        Ok(github) => Arc::new(github),
        Err(error) => {
            eprintln!("mergeable: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let app_client = match AppJwtClient::new(
        &config.github_base_url,
        &config.app_id,
        &config.private_key_pem,
    ) {
        Ok(app_client) => Arc::new(app_client),
        Err(error) => {
            eprintln!("mergeable: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if config.development_mode {
        tracing::info!("In DEVELOPMENT mode.");
    } else {
        tracing::info!("In PRODUCTION mode.");
    }
    let scheduler = if config.scheduler_enabled {
        tracing::info!(
            "Starting scheduler at {} second intervals.",
            config.scheduler_interval_seconds_text
        );
        Some(Scheduler::start(
            Arc::clone(&github),
            Arc::clone(&app_client),
            SchedulerOptions::from_env(config.scheduler_interval_milliseconds),
        ))
    } else {
        tracing::info!("Scheduler: off!");
        None
    };
    let state = ServerState {
        github,
        app_client,
        scheduler,
        webhook_secret: Arc::new(config.webhook_secret.clone()),
        metrics,
    };
    let app = server::router(&config, state);
    let address = format!("{}:{}", config.host, config.port);
    let listener = match tokio::net::TcpListener::bind(&address).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("mergeable: cannot listen on {address}: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    tracing::info!("Listening on http://{address}");
    if let Err(error) = axum::serve(listener, app).await {
        eprintln!("mergeable: server error: {error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
