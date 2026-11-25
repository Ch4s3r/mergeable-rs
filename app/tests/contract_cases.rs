use mergeable::app_config::{AppConfig, Environment};
use mergeable::server;
use replay::case::Case;
use replay::contract::{self, Outcome};
use replay::mock::MockServer;
use std::path::PathBuf;

fn case_path(case_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../app-cases/cases")
        .join(format!("{case_name}.json"))
}

async fn run_contract_case(case_name: &str) {
    let case = Case::load(&case_path(case_name))
        .unwrap_or_else(|error| panic!("loading case {case_name}: {error:#}"));
    let mock = MockServer::start()
        .await
        .unwrap_or_else(|error| panic!("starting the mock GitHub API: {error:#}"));
    let environment: Environment = contract::in_process_app_environment(mock.port, &case)
        .unwrap_or_else(|error| panic!("building the app environment: {error:#}"))
        .into_iter()
        .collect();
    let config = AppConfig::from_environment(&environment)
        .unwrap_or_else(|error| panic!("configuring the app: {error}"));
    let webhook_path = config.webhook_path.clone();
    let running_app = server::start(config)
        .await
        .unwrap_or_else(|error| panic!("starting the app: {error}"));
    let case_run = contract::exercise_case(
        &mock,
        running_app.address().port(),
        &webhook_path,
        &case,
        contract::case_quiet_period(&case, contract::DEFAULT_QUIET_PERIOD),
    )
    .await;
    let shutdown = running_app.shutdown().await;
    mock.stop();
    let case_run = case_run.unwrap_or_else(|error| panic!("running case {case_name}: {error:#}"));
    shutdown.unwrap_or_else(|error| panic!("stopping the app: {error}"));
    let (outcome, detail) = contract::judge(&case, &case_run);
    if outcome == Outcome::Fail && case.definition.known_diff.is_none() {
        let detail = detail.unwrap_or_default();
        let texts = contract::comparable_texts(&case, &case_run);
        pretty_assertions::assert_eq!(
            texts.expected,
            texts.actual,
            "case {case_name} differs from its recording: {detail}"
        );
        panic!("case {case_name} differs from its recording: {detail}");
    }
}

include!(concat!(env!("OUT_DIR"), "/contract_cases.rs"));
