use crate::app_process::{AppProcess, AppProgram};
use crate::case::Case;
use crate::mock::{MockServer, Scenario};
use crate::webhook::{Webhook, WebhookSender};
use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const WARM_UP_ROUNDS: usize = 3;

pub struct BenchSettings {
    pub requests: usize,
    pub concurrency: usize,
}

#[derive(Debug, Serialize)]
pub struct BenchReport {
    pub app: String,
    pub cases: usize,
    pub requests: usize,
    pub concurrency: usize,
    pub failed_webhooks: usize,
    pub webhook_statuses: BTreeMap<String, usize>,
    pub startup_milliseconds: f64,
    pub duration_seconds: f64,
    pub webhooks_per_second: f64,
    pub latency_p50_milliseconds: f64,
    pub latency_p95_milliseconds: f64,
    pub latency_p99_milliseconds: f64,
    pub github_calls_per_second: f64,
    pub peak_rss_megabytes: f64,
}

struct RepositoryRename {
    old_full_name: String,
    new_full_name: String,
    old_repository: String,
    new_repository: String,
}

impl RepositoryRename {
    fn for_case(case: &Case, index: usize) -> Option<RepositoryRename> {
        let old_full_name = case.definition.repository_full_name();
        let (owner, old_name) = old_full_name.split_once('/')?;
        let new_name = format!("{old_name}-bench{index}");
        Some(RepositoryRename {
            new_full_name: format!("{owner}/{new_name}"),
            old_repository: old_name.to_owned(),
            new_repository: new_name,
            old_full_name,
        })
    }

    fn rename_in_payload(&self, value: &mut Value) {
        match value {
            Value::Object(fields) => {
                if fields.get("full_name").and_then(Value::as_str) == Some(&self.old_full_name) {
                    fields.insert(
                        "full_name".to_owned(),
                        Value::String(self.new_full_name.clone()),
                    );
                    if fields.get("name").and_then(Value::as_str) == Some(&self.old_repository) {
                        fields.insert(
                            "name".to_owned(),
                            Value::String(self.new_repository.clone()),
                        );
                    }
                }
                fields
                    .values_mut()
                    .for_each(|item| self.rename_in_payload(item));
            }
            Value::Array(items) => items
                .iter_mut()
                .for_each(|item| self.rename_in_payload(item)),
            _ => {}
        }
    }

    fn rename_in_path(&self, path: &str) -> String {
        let old_prefix = format!("/repos/{}", self.old_full_name);
        match path.strip_prefix(&old_prefix) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                format!("/repos/{}{rest}", self.new_full_name)
            }
            _ => path.to_owned(),
        }
    }

    fn rename_in_file_key(&self, key: &str) -> String {
        match key.strip_prefix(&format!("{}:", self.old_full_name)) {
            Some(file_path) => format!("{}:{file_path}", self.new_full_name),
            None => key.to_owned(),
        }
    }

    fn rename_scenario(&self, scenario: Scenario) -> Scenario {
        Scenario {
            home_repository: self.new_full_name.clone(),
            stubs: scenario
                .stubs
                .into_iter()
                .map(|mut stub| {
                    stub.path = self.rename_in_path(&stub.path);
                    stub
                })
                .collect(),
            files: scenario
                .files
                .into_iter()
                .map(|(key, content)| (self.rename_in_file_key(&key), content))
                .collect(),
        }
    }
}

fn bench_inputs(cases: &[Case]) -> Result<(Vec<Scenario>, Vec<Webhook>)> {
    let mut scenarios = Vec::with_capacity(cases.len());
    let mut webhooks = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        let mut payload = case.definition.payload.clone();
        let mut scenario = Scenario::from_case(&case.definition);
        if let Some(rename) = RepositoryRename::for_case(case, index) {
            rename.rename_in_payload(&mut payload);
            scenario = rename.rename_scenario(scenario);
        }
        scenarios.push(scenario);
        webhooks.push(Webhook {
            event: case.definition.event.clone(),
            delivery_id: case.definition.delivery_id(),
            body: serde_json::to_vec(&payload)?,
        });
    }
    Ok((scenarios, webhooks))
}

async fn resident_set_kilobytes(process_id: u32) -> Option<u64> {
    let output = tokio::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &process_id.to_string()])
        .output()
        .await
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn start_peak_memory_sampler(
    process_id: Option<u32>,
) -> (Arc<AtomicU64>, tokio::task::JoinHandle<()>) {
    let peak_kilobytes = Arc::new(AtomicU64::new(0));
    let sampler_peak = peak_kilobytes.clone();
    let sampler = tokio::spawn(async move {
        let Some(process_id) = process_id else {
            return;
        };
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        loop {
            ticker.tick().await;
            if let Some(kilobytes) = resident_set_kilobytes(process_id).await {
                sampler_peak.fetch_max(kilobytes, Ordering::Relaxed);
            }
        }
    });
    (peak_kilobytes, sampler)
}

fn is_success(status: u16) -> bool {
    (200..300).contains(&status)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "request, call and kilobyte counters stay far below 2^53, so the float is exact for reporting"
)]
fn counter_as_float(counter: u64) -> f64 {
    counter as f64
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "nearest-rank of a non-negative percentile over a sample count far below 2^53, clamped to the sample count right after"
)]
fn percentile_milliseconds(sorted_latencies: &[Duration], percentile: f64) -> f64 {
    if sorted_latencies.is_empty() {
        return 0.0;
    }
    let rank = (percentile * sorted_latencies.len() as f64).ceil() as usize;
    let index = rank.clamp(1, sorted_latencies.len()) - 1;
    sorted_latencies[index].as_secs_f64() * 1000.0
}

async fn send_until_success(
    sender: &WebhookSender,
    app: &mut AppProcess,
    webhook: &Webhook,
) -> Result<()> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        app.ensure_running()?;
        if let Ok(status) = sender.send(app.port, &app.webhook_path, webhook).await
            && is_success(status)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("no successful webhook within {STARTUP_TIMEOUT:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub async fn bench(
    program: &AppProgram,
    cases: &[Case],
    settings: &BenchSettings,
    log_path: &Path,
) -> Result<BenchReport> {
    let Some(first_case) = cases.first() else {
        bail!("no cases selected");
    };
    if cases
        .iter()
        .any(|case| case.definition.env != first_case.definition.env)
    {
        eprintln!(
            "WARNING: cases use different env; benchmarking with the env of {}",
            first_case.definition.name
        );
    }
    let (scenarios, webhooks) = bench_inputs(cases)?;
    let mock = MockServer::start().await?;
    mock.github.load_scenarios(scenarios, false);
    let sender = WebhookSender::new()?;
    let mut app = AppProcess::start(program, mock.port, &first_case.definition.env, log_path)?;
    let (peak_kilobytes, sampler) = start_peak_memory_sampler(app.process_id());
    app.wait_until_listening(STARTUP_TIMEOUT).await?;
    send_until_success(&sender, &mut app, &webhooks[0]).await?;
    let startup = app.started_at.elapsed();
    for _ in 0..WARM_UP_ROUNDS {
        for webhook in &webhooks {
            sender.send(app.port, &app.webhook_path, webhook).await?;
        }
    }
    let webhooks = Arc::new(webhooks);
    let concurrency_limit = Arc::new(Semaphore::new(settings.concurrency.max(1)));
    let calls_before = mock.github.requests_served();
    let measurement_started_at = Instant::now();
    let mut in_flight = JoinSet::new();
    for request_index in 0..settings.requests {
        let permit = concurrency_limit.clone().acquire_owned().await?;
        let sender = sender.clone();
        let webhooks = webhooks.clone();
        let port = app.port;
        let webhook_path = app.webhook_path.clone();
        in_flight.spawn(async move {
            let webhook = &webhooks[request_index % webhooks.len()];
            let sent_at = Instant::now();
            let status = sender.send(port, &webhook_path, webhook).await;
            drop(permit);
            (sent_at.elapsed(), status.ok())
        });
    }
    let mut latencies = Vec::with_capacity(settings.requests);
    let mut failed_webhooks = 0;
    let mut webhook_statuses = BTreeMap::new();
    while let Some(joined) = in_flight.join_next().await {
        let (latency, status) = joined?;
        latencies.push(latency);
        if !status.is_some_and(is_success) {
            failed_webhooks += 1;
        }
        let status_label = status.map_or_else(|| "no response".to_owned(), |code| code.to_string());
        *webhook_statuses.entry(status_label).or_insert(0) += 1;
    }
    let duration = measurement_started_at.elapsed();
    let github_calls = mock.github.requests_served() - calls_before;
    sampler.abort();
    app.stop().await;
    mock.stop();
    latencies.sort();
    let duration_seconds = duration.as_secs_f64();
    Ok(BenchReport {
        app: program.label(),
        cases: cases.len(),
        requests: settings.requests,
        concurrency: settings.concurrency,
        failed_webhooks,
        webhook_statuses,
        startup_milliseconds: startup.as_secs_f64() * 1000.0,
        duration_seconds,
        webhooks_per_second: counter_as_float(u64::try_from(settings.requests).unwrap_or(u64::MAX))
            / duration_seconds,
        latency_p50_milliseconds: percentile_milliseconds(&latencies, 0.50),
        latency_p95_milliseconds: percentile_milliseconds(&latencies, 0.95),
        latency_p99_milliseconds: percentile_milliseconds(&latencies, 0.99),
        github_calls_per_second: counter_as_float(github_calls) / duration_seconds,
        peak_rss_megabytes: counter_as_float(peak_kilobytes.load(Ordering::Relaxed)) / 1024.0,
    })
}

pub fn print_bench_table(report: &BenchReport) {
    let rows = [
        ("app", report.app.clone()),
        (
            "cases / requests / concurrency",
            format!(
                "{} / {} / {}",
                report.cases, report.requests, report.concurrency
            ),
        ),
        ("failed webhooks", report.failed_webhooks.to_string()),
        (
            "webhook statuses",
            report
                .webhook_statuses
                .iter()
                .map(|(status, count)| format!("{status}: {count}"))
                .collect::<Vec<String>>()
                .join(", "),
        ),
        (
            "startup to first webhook",
            format!("{:.0} ms", report.startup_milliseconds),
        ),
        (
            "throughput",
            format!("{:.1} webhooks/s", report.webhooks_per_second),
        ),
        (
            "latency p50 / p95 / p99",
            format!(
                "{:.1} / {:.1} / {:.1} ms",
                report.latency_p50_milliseconds,
                report.latency_p95_milliseconds,
                report.latency_p99_milliseconds
            ),
        ),
        (
            "GitHub calls served",
            format!("{:.1} /s", report.github_calls_per_second),
        ),
        ("peak RSS", format!("{:.1} MB", report.peak_rss_megabytes)),
    ];
    for (label, value) in rows {
        println!("{label:<32} {value}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bench_rename_gives_each_case_its_own_repository() {
        let rename = RepositoryRename {
            old_full_name: "acme/app".to_owned(),
            new_full_name: "acme/app-bench1".to_owned(),
            old_repository: "app".to_owned(),
            new_repository: "app-bench1".to_owned(),
        };
        let mut payload = json!({
            "repository": {"name": "app", "full_name": "acme/app", "owner": {"login": "acme"}},
            "pull_request": {"head": {"repo": {"name": "app", "full_name": "acme/app"}}}
        });
        rename.rename_in_payload(&mut payload);
        assert_eq!(payload["repository"]["name"], "app-bench1");
        assert_eq!(
            payload["pull_request"]["head"]["repo"]["full_name"],
            "acme/app-bench1"
        );
        assert_eq!(
            rename.rename_in_path("/repos/acme/app/check-runs"),
            "/repos/acme/app-bench1/check-runs"
        );
        assert_eq!(
            rename.rename_in_path("/repos/acme/apple"),
            "/repos/acme/apple"
        );
        assert_eq!(
            rename.rename_in_file_key("acme/app:.github/x.yml"),
            "acme/app-bench1:.github/x.yml"
        );
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let latencies: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile_milliseconds(&latencies, 0.50), 50.0);
        assert_eq!(percentile_milliseconds(&latencies, 0.99), 99.0);
    }
}
