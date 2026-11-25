use crate::contract::case::Stub;
use crate::contract::mock::{MockServer, Scenario};
use crate::contract::runner;
use crate::contract::webhook::{Webhook, WebhookSender};
use anyhow::{Result, bail};
use mergeable::app_config::{AppConfig, Environment};
use mergeable::server::{self, RunningServer};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const SCHEDULER_INTERVAL_SECONDS: &str = "0.2";
const FAST_RESYNC_INTERVAL_SECONDS: &str = "0.3";
const SLOW_RESYNC_INTERVAL_SECONDS: &str = "3600";
const WAIT_LIMIT: Duration = Duration::from_secs(15);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const SETTLE_TIME: Duration = Duration::from_millis(800);
const OBSERVATION_WINDOW: Duration = Duration::from_millis(1500);

struct ScheduledApp {
    mock: MockServer,
    app: RunningServer,
    webhook_path: String,
}

fn installation(installation_id: u64) -> Value {
    json!({"id": installation_id, "account": {"login": "acme", "type": "Organization"}})
}

fn repository(repository_id: u64, name: &str) -> Value {
    json!({"id": repository_id, "name": name, "full_name": format!("acme/{name}"), "owner": {"login": "acme"}})
}

fn stub(path: &str, status: u16, body: &Value) -> Result<Stub> {
    Ok(serde_json::from_value(
        json!({"method": "GET", "path": path, "status": status, "body": body}),
    )?)
}

fn github_listing(installations: &[Value], repositories: &[Value]) -> Result<Scenario> {
    Ok(Scenario {
        home_repository: String::new(),
        stubs: vec![
            stub("/app/installations", 200, &json!(installations))?,
            stub(
                "/installation/repositories",
                200,
                &json!({"total_count": repositories.len(), "repositories": repositories}),
            )?,
        ],
        files: BTreeMap::new(),
    })
}

fn failing_installation_listing() -> Result<Scenario> {
    Ok(Scenario {
        home_repository: String::new(),
        stubs: vec![stub(
            "/app/installations",
            500,
            &json!({"message": "Server Error"}),
        )?],
        files: BTreeMap::new(),
    })
}

async fn start_scheduled_app(
    resync_interval_seconds: &str,
    listing: Scenario,
) -> Result<ScheduledApp> {
    let mock = MockServer::start().await?;
    mock.github.load_scenario(listing);
    let scheduler_variables = BTreeMap::from(
        [
            ("MERGEABLE_SCHEDULER", "true"),
            ("MERGEABLE_SCHEDULER_INTERVAL", SCHEDULER_INTERVAL_SECONDS),
            (
                "MERGEABLE_INSTALLATION_RESYNC_INTERVAL",
                resync_interval_seconds,
            ),
            ("DISABLE_DELAY", "true"),
            ("GITHUB_THROTTLE", "false"),
        ]
        .map(|(name, value)| (name.to_owned(), value.to_owned())),
    );
    let environment: Environment = runner::app_environment_with(mock.port, &scheduler_variables)?
        .into_iter()
        .collect();
    let config = AppConfig::from_environment(&environment).map_err(anyhow::Error::msg)?;
    let webhook_path = config.webhook_path.clone();
    let app = server::start(config).await?;
    Ok(ScheduledApp {
        mock,
        app,
        webhook_path,
    })
}

fn runs_of(calls: &[Value], repository_name: &str) -> usize {
    let repository_path = format!("/repos/acme/{repository_name}/");
    calls
        .iter()
        .filter(|call| {
            call.get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.starts_with(&repository_path))
        })
        .count()
}

impl ScheduledApp {
    async fn wait_for_run_of(&self, repository_name: &str) -> Result<()> {
        let started = Instant::now();
        while runs_of(&self.mock.github.recorded_calls(), repository_name) == 0 {
            if started.elapsed() >= WAIT_LIMIT {
                bail!("acme/{repository_name} was never run by the scheduler");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        Ok(())
    }

    async fn change_listing_and_wait_for_resync(&self, listing: Scenario) -> Result<()> {
        self.mock.github.load_scenario(listing);
        let requests_before = self.mock.github.installation_list_request_count();
        let started = Instant::now();
        while self.mock.github.installation_list_request_count() <= requests_before {
            if started.elapsed() >= WAIT_LIMIT {
                bail!("no resync listed the installations again");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        tokio::time::sleep(SETTLE_TIME).await;
        Ok(())
    }

    async fn send_webhook(&self, event: &str, payload: &Value) -> Result<()> {
        let webhook = Webhook {
            event: event.to_owned(),
            delivery_id: format!("resync-{event}"),
            body: serde_json::to_vec(payload)?,
        };
        let status = WebhookSender::new()?
            .send(self.app.address().port(), &self.webhook_path, &webhook)
            .await?;
        if status != 200 {
            bail!("webhook {event} answered {status}");
        }
        tokio::time::sleep(SETTLE_TIME).await;
        Ok(())
    }

    async fn runs_during_observation_window(&self, repository_names: &[&str]) -> Vec<usize> {
        self.mock.github.take_calls();
        tokio::time::sleep(OBSERVATION_WINDOW).await;
        let calls = self.mock.github.take_calls();
        repository_names
            .iter()
            .map(|repository_name| runs_of(&calls, repository_name))
            .collect()
    }

    async fn stop(self) -> Result<()> {
        self.app.shutdown().await?;
        self.mock.stop();
        Ok(())
    }
}

#[tokio::test]
async fn repository_added_while_its_webhook_was_missed_is_scheduled_after_the_next_resync()
-> Result<()> {
    let scheduled_app = start_scheduled_app(
        FAST_RESYNC_INTERVAL_SECONDS,
        github_listing(&[installation(1)], &[repository(10, "app")])?,
    )
    .await?;
    scheduled_app.wait_for_run_of("app").await?;
    scheduled_app
        .change_listing_and_wait_for_resync(github_listing(
            &[installation(1)],
            &[repository(10, "app"), repository(11, "web")],
        )?)
        .await?;
    assert_eq!(
        scheduled_app
            .runs_during_observation_window(&["app", "web"])
            .await
            .iter()
            .map(|runs| *runs > 0)
            .collect::<Vec<_>>(),
        [true, true]
    );
    scheduled_app.stop().await
}

#[tokio::test]
async fn repository_removed_while_its_webhook_was_missed_stops_being_scheduled_after_the_next_resync()
-> Result<()> {
    let scheduled_app = start_scheduled_app(
        FAST_RESYNC_INTERVAL_SECONDS,
        github_listing(
            &[installation(1)],
            &[repository(10, "app"), repository(11, "web")],
        )?,
    )
    .await?;
    scheduled_app.wait_for_run_of("app").await?;
    scheduled_app.wait_for_run_of("web").await?;
    scheduled_app
        .change_listing_and_wait_for_resync(github_listing(
            &[installation(1)],
            &[repository(11, "web")],
        )?)
        .await?;
    let [app_runs, web_runs] = scheduled_app
        .runs_during_observation_window(&["app", "web"])
        .await[..]
    else {
        bail!("expected run counts for two repositories");
    };
    assert_eq!(app_runs, 0);
    assert!(web_runs > 0);
    scheduled_app.stop().await
}

#[tokio::test]
async fn uninstalled_installation_stops_being_scheduled_after_the_next_resync() -> Result<()> {
    let scheduled_app = start_scheduled_app(
        FAST_RESYNC_INTERVAL_SECONDS,
        github_listing(&[installation(1)], &[repository(10, "app")])?,
    )
    .await?;
    scheduled_app.wait_for_run_of("app").await?;
    scheduled_app
        .change_listing_and_wait_for_resync(github_listing(&[], &[])?)
        .await?;
    assert_eq!(
        scheduled_app.runs_during_observation_window(&["app"]).await,
        [0]
    );
    scheduled_app.stop().await
}

#[tokio::test]
async fn failed_resync_keeps_the_repositories_scheduled() -> Result<()> {
    let scheduled_app = start_scheduled_app(
        FAST_RESYNC_INTERVAL_SECONDS,
        github_listing(&[installation(1)], &[repository(10, "app")])?,
    )
    .await?;
    scheduled_app.wait_for_run_of("app").await?;
    scheduled_app
        .change_listing_and_wait_for_resync(failing_installation_listing()?)
        .await?;
    assert!(scheduled_app.runs_during_observation_window(&["app"]).await[0] > 0);
    scheduled_app.stop().await
}

#[tokio::test]
async fn installation_deleted_webhook_stops_scheduling_its_repositories() -> Result<()> {
    let scheduled_app = start_scheduled_app(
        SLOW_RESYNC_INTERVAL_SECONDS,
        github_listing(&[installation(1)], &[repository(10, "app")])?,
    )
    .await?;
    scheduled_app.wait_for_run_of("app").await?;
    scheduled_app
        .send_webhook(
            "installation",
            &json!({"action": "deleted", "installation": installation(1), "sender": {"login": "alice", "type": "User"}}),
        )
        .await?;
    assert_eq!(
        scheduled_app.runs_during_observation_window(&["app"]).await,
        [0]
    );
    scheduled_app.stop().await
}

#[tokio::test]
async fn installation_repositories_removed_webhook_stops_scheduling_only_the_removed_repository()
-> Result<()> {
    let scheduled_app = start_scheduled_app(
        SLOW_RESYNC_INTERVAL_SECONDS,
        github_listing(
            &[installation(1)],
            &[repository(10, "app"), repository(11, "web")],
        )?,
    )
    .await?;
    scheduled_app.wait_for_run_of("app").await?;
    scheduled_app.wait_for_run_of("web").await?;
    scheduled_app
        .send_webhook(
            "installation_repositories",
            &json!({
                "action": "removed",
                "installation": installation(1),
                "repositories_added": [],
                "repositories_removed": [repository(10, "app")],
                "sender": {"login": "alice", "type": "User"},
            }),
        )
        .await?;
    let [app_runs, web_runs] = scheduled_app
        .runs_during_observation_window(&["app", "web"])
        .await[..]
    else {
        bail!("expected run counts for two repositories");
    };
    assert_eq!(app_runs, 0);
    assert!(web_runs > 0);
    scheduled_app.stop().await
}
