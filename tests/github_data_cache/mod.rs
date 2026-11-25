use crate::contract::case::Case;
use crate::contract::mock::MockServer;
use crate::contract::runner::{self, Outcome};
use anyhow::{Context as _, Result, bail};
use mergeable::app_config::{AppConfig, Environment};
use mergeable::server::{self, RunningServer};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const VALKEY_SERVER: &str = "valkey-server";
const VALKEY_STARTUP_LIMIT: Duration = Duration::from_secs(10);

fn recorded_case(case_name: &str, extra_environment: &[(&str, &str)]) -> Result<Case> {
    let case_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cases")
        .join(format!("{case_name}.json"));
    let mut case = Case::load(&case_path)?;
    case.definition.env.extend(
        extra_environment
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned())),
    );
    Ok(case)
}

async fn start_app(mock: &MockServer, case: &Case) -> Result<(RunningServer, String)> {
    let environment: Environment = runner::in_process_app_environment(mock.port, case)?
        .into_iter()
        .collect();
    let config = AppConfig::from_environment(&environment).map_err(anyhow::Error::msg)?;
    let webhook_path = config.webhook_path.clone();
    let running_app = server::start(config).await?;
    Ok((running_app, webhook_path))
}

async fn deliver(
    mock: &MockServer,
    app: &RunningServer,
    webhook_path: &str,
    case: &Case,
) -> Result<Vec<String>> {
    let case_run = runner::exercise_case(
        mock,
        app.address().port(),
        webhook_path,
        case,
        runner::case_quiet_period(case, runner::DEFAULT_QUIET_PERIOD),
    )
    .await?;
    if case_run.status != 200 {
        bail!("webhook answered {}", case_run.status);
    }
    Ok(case_run.calls.iter().map(call_summary).collect())
}

async fn deliver_matching_the_recording(
    mock: &MockServer,
    app: &RunningServer,
    webhook_path: &str,
    case: &Case,
) -> Result<Vec<String>> {
    let case_run = runner::exercise_case(
        mock,
        app.address().port(),
        webhook_path,
        case,
        runner::case_quiet_period(case, runner::DEFAULT_QUIET_PERIOD),
    )
    .await?;
    let (outcome, detail) = runner::judge(case, &case_run);
    if outcome == Outcome::Fail && case.definition.known_diff.is_none() {
        let texts = runner::comparable_texts(case, &case_run);
        bail!(
            "a cold cache must reproduce the recording: {}\nexpected {}\nactual {}",
            detail.unwrap_or_default(),
            texts.expected,
            texts.actual
        );
    }
    Ok(case_run.calls.iter().map(call_summary).collect())
}

fn call_summary(call: &Value) -> String {
    format!(
        "{} {}",
        call.get("method")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        call.get("path").and_then(Value::as_str).unwrap_or_default()
    )
}

struct RepeatedDelivery {
    first_calls: Vec<String>,
    second_calls: Vec<String>,
}

impl RepeatedDelivery {
    fn calls_saved(&self) -> Vec<String> {
        let mut remaining = self.second_calls.clone();
        self.first_calls
            .iter()
            .filter(|call| {
                if let Some(position) = remaining.iter().position(|other| other == *call) {
                    remaining.remove(position);
                    false
                } else {
                    true
                }
            })
            .cloned()
            .collect()
    }
}

async fn deliver_twice(
    case_name: &str,
    extra_environment: &[(&str, &str)],
) -> Result<RepeatedDelivery> {
    let case = recorded_case(case_name, extra_environment)?;
    let mock = MockServer::start().await?;
    let (app, webhook_path) = start_app(&mock, &case).await?;
    let first_calls = deliver_matching_the_recording(&mock, &app, &webhook_path, &case).await?;
    let second_calls = deliver(&mock, &app, &webhook_path, &case).await?;
    app.shutdown().await?;
    mock.stop();
    Ok(RepeatedDelivery {
        first_calls,
        second_calls,
    })
}

const MEMORY_GITHUB_DATA_CACHE: [(&str, &str); 1] = [("USE_GITHUB_DATA_CACHE", "true")];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_the_github_data_cache_a_repeated_webhook_fetches_everything_again() -> Result<()> {
    let delivery = deliver_twice("filter-author-team-pass", &[]).await?;
    assert_eq!(delivery.calls_saved(), Vec::<String>::new());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_webhook_reuses_the_cached_team_membership() -> Result<()> {
    let delivery = deliver_twice("filter-author-team-pass", &MEMORY_GITHUB_DATA_CACHE).await?;
    assert_eq!(
        delivery.calls_saved(),
        vec!["GET /orgs/acme/teams/core/memberships/alice"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_webhook_reuses_the_cached_team_member_list() -> Result<()> {
    let delivery = deliver_twice(
        "validator-author-one-of-team-members",
        &MEMORY_GITHUB_DATA_CACHE,
    )
    .await?;
    assert!(
        !delivery.calls_saved().is_empty()
            && delivery
                .calls_saved()
                .iter()
                .all(|call| call.ends_with("/members")),
        "saved calls: {:?}",
        delivery.calls_saved()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_webhook_reuses_the_cached_collaborators() -> Result<()> {
    let delivery =
        deliver_twice("action-request-review-reviewers", &MEMORY_GITHUB_DATA_CACHE).await?;
    assert_eq!(
        delivery.calls_saved(),
        vec!["GET /repos/acme/app/collaborators"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_webhook_reuses_the_cached_repository_topics() -> Result<()> {
    let delivery =
        deliver_twice("filter-repository-topics-pass", &MEMORY_GITHUB_DATA_CACHE).await?;
    assert_eq!(delivery.calls_saved(), vec!["GET /repos/acme/app/topics"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_webhook_reuses_the_cached_organization_default_config() -> Result<()> {
    let delivery =
        deliver_twice("config-env-use-org-as-default", &MEMORY_GITHUB_DATA_CACHE).await?;
    assert_eq!(
        delivery.calls_saved(),
        vec!["GET /repos/acme/.github/contents/.github/mergeable.yml"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cache_hits_and_misses_are_exposed_on_the_metrics_endpoint() -> Result<()> {
    let case = recorded_case(
        "filter-author-team-pass",
        &[
            ("USE_GITHUB_DATA_CACHE", "true"),
            ("ENABLE_METRICS_ENDPOINT", "true"),
        ],
    )?;
    let mock = MockServer::start().await?;
    let (app, webhook_path) = start_app(&mock, &case).await?;
    deliver(&mock, &app, &webhook_path, &case).await?;
    deliver(&mock, &app, &webhook_path, &case).await?;
    let metrics = reqwest::get(format!("http://{}/metrics", app.address()))
        .await?
        .text()
        .await?;
    app.shutdown().await?;
    mock.stop();
    assert!(
        metrics.contains(r#"cache_lookups_total{cache="team_memberships",result="miss"} 1"#),
        "{metrics}"
    );
    assert!(
        metrics.contains(r#"cache_lookups_total{cache="team_memberships",result="hit"} 1"#),
        "{metrics}"
    );
    Ok(())
}

fn executable_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

struct ValkeyServer {
    process: Child,
    port: u16,
}

impl ValkeyServer {
    fn start(executable: &Path) -> Result<ValkeyServer> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let process = Command::new(executable)
            .args(["--port", &port.to_string(), "--bind", "127.0.0.1"])
            .args(["--save", "", "--appendonly", "no"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("starting valkey-server")?;
        let server = ValkeyServer { process, port };
        let started_at = Instant::now();
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            if started_at.elapsed() > VALKEY_STARTUP_LIMIT {
                bail!("valkey-server did not listen on port {port}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(server)
    }
}

impl Drop for ValkeyServer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replicas_share_cached_github_data_through_valkey() -> Result<()> {
    let Some(valkey_executable) = executable_on_path(VALKEY_SERVER) else {
        eprintln!("skipped: {VALKEY_SERVER} is not on PATH (run inside devenv shell)");
        return Ok(());
    };
    let valkey = ValkeyServer::start(&valkey_executable)?;
    let valkey_port = valkey.port.to_string();
    let case = recorded_case(
        "filter-author-team-pass",
        &[
            ("USE_GITHUB_DATA_CACHE", "true"),
            ("CACHE_STORAGE", "valkey"),
            ("CACHE_REDIS_HOST", "127.0.0.1"),
            ("CACHE_REDIS_PORT", &valkey_port),
        ],
    )?;
    let mock = MockServer::start().await?;
    let (first_replica, webhook_path) = start_app(&mock, &case).await?;
    let (second_replica, _) = start_app(&mock, &case).await?;
    let first_calls =
        deliver_matching_the_recording(&mock, &first_replica, &webhook_path, &case).await?;
    let second_calls = deliver(&mock, &second_replica, &webhook_path, &case).await?;
    first_replica.shutdown().await?;
    second_replica.shutdown().await?;
    mock.stop();
    let delivery = RepeatedDelivery {
        first_calls,
        second_calls,
    };
    assert_eq!(
        delivery.calls_saved(),
        vec!["GET /orgs/acme/teams/core/memberships/alice"]
    );
    drop(valkey);
    Ok(())
}
