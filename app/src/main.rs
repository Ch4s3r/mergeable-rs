use mergeable::app_config::AppConfig;
use mergeable::error::Error;
use mergeable::github::app_auth::AppJwtClient;
use mergeable::github::client::{GithubApp, GithubAppSettings};
use mergeable::github::throttle::RequestThrottle;
use mergeable::logging;
use mergeable::metrics::Metrics;
use mergeable::scheduler::{Scheduler, SchedulerOptions};
use mergeable::server::{self, ServerState};
use std::fmt;
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Debug)]
enum StartupError {
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

#[tokio::main]
async fn main() -> ExitCode {
    let config = match AppConfig::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("mergeable: {message}");
            return ExitCode::FAILURE;
        }
    };
    logging::init(&config.log_level, config.log_format);
    match serve(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "app.startup.failed");
            eprintln!("mergeable: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config: AppConfig) -> Result<(), StartupError> {
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
        },
        metrics.clone(),
    )?);
    let app_client = Arc::new(AppJwtClient::new(
        &config.github_base_url,
        &config.app_id,
        &config.private_key_pem,
    )?);
    tracing::info!(run_mode = ?config.run_mode, "app.mode");
    let scheduler = if let Some(interval_milliseconds) = config.scheduler_interval_milliseconds {
        tracing::info!(interval_milliseconds, "scheduler.started");
        Some(Scheduler::start(
            Arc::clone(&github),
            Arc::clone(&app_client),
            SchedulerOptions::from_env(interval_milliseconds),
        ))
    } else {
        tracing::info!("scheduler.disabled");
        None
    };
    let address = format!("{}:{}", config.host, config.port);
    let state = ServerState {
        github,
        app_client,
        scheduler,
        webhook_secret: Arc::from(config.webhook_secret.as_str()),
        metrics,
    };
    let app = server::router(&config, state);
    let listener = tokio::net::TcpListener::bind(&address)
        .await
        .map_err(|source| StartupError::Listen {
            address: address.clone(),
            source,
        })?;
    tracing::info!(%address, "server.listening");
    axum::serve(listener, app)
        .await
        .map_err(StartupError::Serve)
}
