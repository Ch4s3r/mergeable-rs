use crate::app_process::{AppProcess, AppProgram};
use crate::case::Case;
use crate::mock::{MockServer, Scenario};
use crate::webhook::{Webhook, WebhookSender};
use anyhow::{Context as _, Result, anyhow};
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const MAXIMUM_CASE_DURATION: Duration = Duration::from_secs(40);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub quiet_period: Duration,
    pub jobs: usize,
    pub reuse_process: bool,
    pub log_directory: PathBuf,
}

impl RunOptions {
    fn log_path(&self, log_name: &str) -> PathBuf {
        self.log_directory.join(format!("{log_name}.log"))
    }
}

pub struct CaseRun {
    pub status: u16,
    pub calls: Vec<Value>,
    pub settled: bool,
}

pub fn webhook_of_case(case: &Case) -> Result<Webhook> {
    Ok(Webhook {
        event: case.definition.event.clone(),
        delivery_id: case.definition.delivery_id(),
        body: serde_json::to_vec(&case.definition.payload)?,
    })
}

fn case_quiet_period(case: &Case, default_quiet_period: Duration) -> Duration {
    case.definition
        .quiet_ms
        .map(Duration::from_millis)
        .map_or(default_quiet_period, |case_period| {
            case_period.max(default_quiet_period)
        })
}

async fn wait_for_quiet_mock(
    mock: &MockServer,
    webhook_answered_at: Instant,
    quiet_period: Duration,
) -> bool {
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let last_activity = mock.github.last_request_at().max(webhook_answered_at);
        if last_activity.elapsed() >= quiet_period {
            return true;
        }
        if webhook_answered_at.elapsed() >= MAXIMUM_CASE_DURATION {
            return false;
        }
    }
}

async fn exercise_case(
    mock: &MockServer,
    app: &AppProcess,
    case: &Case,
    quiet_period: Duration,
) -> Result<CaseRun> {
    mock.github
        .load_scenarios(vec![Scenario::from_case(&case.definition)], true);
    let status = WebhookSender::new()?
        .send(app.port, &app.webhook_path, &webhook_of_case(case)?)
        .await
        .with_context(|| format!("sending webhook of case {}", case.definition.name))?;
    let settled = wait_for_quiet_mock(mock, Instant::now(), quiet_period).await;
    Ok(CaseRun {
        status,
        calls: mock.github.take_calls(),
        settled,
    })
}

pub async fn run_case_in_fresh_process(
    program: &AppProgram,
    case: &Case,
    options: &RunOptions,
    log_name: &str,
) -> Result<CaseRun> {
    let mock = MockServer::start().await?;
    let mut app = AppProcess::start(
        program,
        mock.port,
        &case.definition.env,
        &options.log_path(log_name),
    )?;
    let outcome = match app.wait_until_listening(STARTUP_TIMEOUT).await {
        Ok(()) => {
            exercise_case(
                &mock,
                &app,
                case,
                case_quiet_period(case, options.quiet_period),
            )
            .await
        }
        Err(error) => Err(error),
    };
    app.stop().await;
    mock.stop();
    outcome
}

pub async fn run_cases_in_one_process(
    program: &AppProgram,
    cases: &[Case],
    options: &RunOptions,
    log_name: &str,
) -> Result<Vec<Result<CaseRun>>> {
    let mock = MockServer::start().await?;
    let mut running_app: Option<AppProcess> = None;
    let mut case_runs = Vec::with_capacity(cases.len());
    for case in cases {
        let app = match running_app.take() {
            Some(app) if app.case_env == case.definition.env => app,
            previous_app => {
                if let Some(previous_app) = previous_app {
                    previous_app.stop().await;
                }
                let mut app = AppProcess::start(
                    program,
                    mock.port,
                    &case.definition.env,
                    &options.log_path(log_name),
                )?;
                app.wait_until_listening(STARTUP_TIMEOUT).await?;
                app
            }
        };
        let app = running_app.insert(app);
        case_runs.push(
            exercise_case(
                &mock,
                app,
                case,
                case_quiet_period(case, options.quiet_period),
            )
            .await,
        );
    }
    if let Some(app) = running_app {
        app.stop().await;
    }
    mock.stop();
    Ok(case_runs)
}

pub async fn for_each_in_parallel<Item, Output, Work, WorkFuture>(
    items: Vec<Item>,
    jobs: usize,
    work: Work,
) -> Result<Vec<Output>>
where
    Item: Send + 'static,
    Output: Send + 'static,
    Work: Fn(Item) -> WorkFuture + Send + Sync + 'static,
    WorkFuture: Future<Output = Output> + Send + 'static,
{
    let job_slots = Arc::new(Semaphore::new(jobs.max(1)));
    let work = Arc::new(work);
    let item_count = items.len();
    let mut running = JoinSet::new();
    for (index, item) in items.into_iter().enumerate() {
        let job_slots = job_slots.clone();
        let work = work.clone();
        running.spawn(async move {
            let permit = job_slots.acquire_owned().await;
            let output = work(item).await;
            drop(permit);
            (index, output)
        });
    }
    let mut outputs: Vec<Option<Output>> = (0..item_count).map(|_| None).collect();
    while let Some(joined) = running.join_next().await {
        let (index, output) = joined?;
        outputs[index] = Some(output);
    }
    outputs
        .into_iter()
        .map(|output| output.ok_or_else(|| anyhow!("a parallel job produced no result")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parallel_results_keep_input_order() -> Result<()> {
        let delays = vec![30u64, 0, 10, 5];
        let outputs = for_each_in_parallel(delays, 3, |delay| async move {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            delay
        })
        .await?;
        assert_eq!(outputs, vec![30, 0, 10, 5]);
        Ok(())
    }
}
