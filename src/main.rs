use mergeable::app_config::{AppConfig, Environment};
use mergeable::logging;
use mergeable::server;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let config = match AppConfig::from_environment(&Environment::from_process()) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("mergeable: {message}");
            return ExitCode::FAILURE;
        }
    };
    logging::init(&config.log_level, config.log_format);
    let outcome = match server::start(config).await {
        Ok(running_server) => running_server.wait().await,
        Err(error) => Err(error),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "app.startup.failed");
            eprintln!("mergeable: {error}");
            ExitCode::FAILURE
        }
    }
}
