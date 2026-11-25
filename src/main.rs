use mergeable::app_config::{AppConfig, Environment};
use mergeable::logging;
use mergeable::server;
use mergeable::telemetry::Telemetry;
use std::process::ExitCode;

async fn termination_requested() {
    let interrupted = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminated = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut termination) => {
                termination.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminated = std::future::pending::<()>();
    tokio::select! {
        () = interrupted => {}
        () = terminated => {}
    }
    tracing::info!("server.shutdown.requested");
}

async fn flush_and_stop(telemetry: Telemetry) {
    if telemetry.is_exporting() {
        let _ = tokio::task::spawn_blocking(move || telemetry.shutdown()).await;
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let config = match AppConfig::from_environment(&Environment::from_process()) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("mergeable: {message}");
            return ExitCode::FAILURE;
        }
    };
    let telemetry = match Telemetry::start(&config.telemetry) {
        Ok(telemetry) => telemetry,
        Err(message) => {
            eprintln!("mergeable: {message}");
            return ExitCode::FAILURE;
        }
    };
    logging::init(&config.log_level, config.log_format, &telemetry);
    config.telemetry.log_summary();
    let outcome = match server::start(config).await {
        Ok(running_server) => running_server.serve_until(termination_requested()).await,
        Err(error) => Err(error),
    };
    if let Err(error) = &outcome {
        tracing::error!(%error, "app.startup.failed");
        eprintln!("mergeable: {error}");
    }
    flush_and_stop(telemetry).await;
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
