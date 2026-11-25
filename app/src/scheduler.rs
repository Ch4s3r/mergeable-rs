use crate::app_config::Environment;
use crate::app_state::AppState;
use crate::context::SharedPayload;
use crate::error::{Error, Result};
use crate::github::app_auth::AppJwtClient;
use crate::github::client::GithubApp;
use crate::json_value;
use crate::server;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const MAXIMUM_TIMER_MILLISECONDS: f64 = 2_147_483_647.0;
const SCHEDULE_EVENT_NAME: &str = "schedule";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRun {
    RandomlyDelayed,
    Immediate,
}

#[derive(Debug, Clone)]
pub struct SchedulerOptions {
    pub first_run: FirstRun,
    pub interval_milliseconds: f64,
    pub ignored_accounts: Vec<String>,
}

impl SchedulerOptions {
    #[must_use]
    pub fn from_environment(environment: &Environment, interval_milliseconds: f64) -> Self {
        SchedulerOptions {
            first_run: if environment.non_empty("DISABLE_DELAY").is_none() {
                FirstRun::RandomlyDelayed
            } else {
                FirstRun::Immediate
            },
            interval_milliseconds,
            ignored_accounts: parse_ignored_accounts(
                environment.value("IGNORED_ACCOUNTS").unwrap_or_default(),
            ),
        }
    }
}

pub fn parse_ignored_accounts(ignored_accounts_text: &str) -> Vec<String> {
    ignored_accounts_text
        .to_lowercase()
        .split(',')
        .map(str::to_string)
        .collect()
}

#[must_use]
pub fn timer_duration(milliseconds: f64) -> Duration {
    let effective = if (1.0..=MAXIMUM_TIMER_MILLISECONDS).contains(&milliseconds) {
        milliseconds
    } else {
        1.0
    };
    Duration::from_secs_f64(effective / 1000.0)
}

#[must_use]
pub fn scheduled_event_payload(installation: &Value, repository: Value) -> Value {
    let mut payload = Map::new();
    payload.insert(
        "action".to_string(),
        Value::String("repository".to_string()),
    );
    payload.insert("installation".to_string(), installation.clone());
    payload.insert("repository".to_string(), repository);
    Value::Object(payload)
}

pub fn is_ignored_installation(
    ignored_accounts: &[String],
    installation: Option<&Value>,
) -> Result<bool> {
    let account = json_value::read(installation, "account")?;
    let login = json_value::read(account, "login")?;
    let lowercase_login = match login {
        Some(Value::String(text)) => text.to_lowercase(),
        Some(Value::Null) | None => {
            json_value::read(login, "toLowerCase")?;
            String::new()
        }
        Some(_) => {
            return Err(Error::type_mismatch(
                "installation.account.login.toLowerCase is not a function",
            ));
        }
    };
    Ok(ignored_accounts.contains(&lowercase_login))
}

fn repository_key(repository: &Value) -> String {
    json_value::to_display_string(json_value::get(repository, "id"))
}

pub struct Scheduler {
    github: Arc<GithubApp>,
    app: Arc<AppState>,
    app_client: Arc<AppJwtClient>,
    options: SchedulerOptions,
    intervals: Mutex<HashMap<String, JoinHandle<()>>>,
    installation_queue: mpsc::UnboundedSender<Value>,
}

impl Scheduler {
    pub fn start(
        github: Arc<GithubApp>,
        app: Arc<AppState>,
        app_client: Arc<AppJwtClient>,
        options: SchedulerOptions,
    ) -> Arc<Scheduler> {
        let (installation_queue, mut queued_installations) = mpsc::unbounded_channel::<Value>();
        let scheduler = Arc::new(Scheduler {
            github,
            app,
            app_client,
            options,
            intervals: Mutex::new(HashMap::new()),
            installation_queue,
        });
        let worker = Arc::clone(&scheduler);
        tokio::spawn(async move {
            while let Some(installation) = queued_installations.recv().await {
                if let Err(error) = worker.schedule_each_repository(Some(&installation)).await {
                    tracing::error!(%error, "scheduler.installation.failed");
                }
            }
        });
        let setup = Arc::clone(&scheduler);
        tokio::spawn(async move {
            if let Err(error) = setup.setup().await {
                tracing::error!(%error, "scheduler.setup.failed");
            }
        });
        scheduler
    }

    #[must_use]
    pub fn listens_to(event_name: &str, payload: &Value) -> bool {
        let action = json_value::as_str(payload.get("action"));
        matches!(
            (event_name, action),
            ("installation", Some("created")) | ("installation_repositories", Some("added"))
        )
    }

    pub async fn handle_webhook(self: Arc<Self>, event_name: &str, payload: &Value) -> Result<()> {
        let installation = json_value::get(payload, "installation");
        if event_name == "installation" {
            return self.schedule_each_repository(installation).await;
        }
        if self.accepts_installation(installation)? {
            self.queue_installation(installation.cloned().unwrap_or(Value::Null));
        }
        Ok(())
    }

    async fn setup(&self) -> Result<()> {
        let installations = self
            .app_client
            .paginate_as_app("/app/installations?per_page=100")
            .await?;
        for installation in installations {
            if self.accepts_installation(Some(&installation))? {
                self.queue_installation(installation);
            }
        }
        Ok(())
    }

    fn accepts_installation(&self, installation: Option<&Value>) -> Result<bool> {
        if is_ignored_installation(&self.options.ignored_accounts, installation)? {
            tracing::info!(installation = ?installation, "scheduler.installation.ignored");
            return Ok(false);
        }
        Ok(true)
    }

    fn queue_installation(&self, installation: Value) {
        if self.installation_queue.send(installation).is_err() {
            tracing::error!("scheduler.queue.closed");
        }
    }

    async fn schedule_each_repository(&self, installation: Option<&Value>) -> Result<()> {
        let installation_id = json_value::read(installation, "id")?.and_then(Value::as_u64);
        let installation = installation.unwrap_or(&Value::Null);
        let client = self.github.installation(installation_id);
        let repositories = client
            .paginate("GET /installation/repositories", json!({"per_page": 100}))
            .await?;
        for repository in repositories {
            self.schedule(installation, repository);
        }
        Ok(())
    }

    fn schedule(&self, installation: &Value, repository: Value) {
        let key = repository_key(&repository);
        let mut intervals = self
            .intervals
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if intervals.contains_key(&key) {
            return;
        }
        let interval_milliseconds = self.options.interval_milliseconds;
        let delay_milliseconds = match self.options.first_run {
            FirstRun::RandomlyDelayed => interval_milliseconds * rand::random::<f64>(),
            FirstRun::Immediate => 0.0,
        };
        tracing::debug!(
            repository = %repository,
            delay_milliseconds,
            interval_milliseconds,
            "scheduler.repository.scheduled"
        );
        let payload = SharedPayload::from(scheduled_event_payload(installation, repository));
        let github = Arc::clone(&self.github);
        let app = Arc::clone(&self.app);
        let timer = tokio::spawn(async move {
            tokio::time::sleep(timer_duration(delay_milliseconds)).await;
            loop {
                let receive_github = Arc::clone(&github);
                let receive_app = Arc::clone(&app);
                let receive_payload = payload.clone();
                tokio::spawn(async move {
                    let outcome = server::run_mergeable(
                        receive_github,
                        receive_app,
                        String::new(),
                        SCHEDULE_EVENT_NAME.to_string(),
                        receive_payload,
                    )
                    .await;
                    if let Err(error) = outcome {
                        tracing::error!(%error, "scheduler.run.failed");
                    }
                });
                tokio::time::sleep(timer_duration(interval_milliseconds)).await;
            }
        });
        intervals.insert(key, timer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_event_carries_the_installation_and_repository_unchanged() {
        let installation = json!({"id": 2, "account": {"login": "octocat"}});
        let repository = json!({"id": 7, "name": "app", "owner": {"login": "octocat"}});
        assert_eq!(
            scheduled_event_payload(&installation, repository.clone()),
            json!({"action": "repository", "installation": installation, "repository": repository})
        );
        let payload = scheduled_event_payload(&installation, repository);
        let keys: Vec<&String> = payload
            .as_object()
            .map(|payload| payload.keys().collect())
            .unwrap_or_default();
        assert_eq!(keys, ["action", "installation", "repository"]);
    }

    #[test]
    fn ignored_accounts_are_lowercased_and_split_by_comma() {
        assert_eq!(
            parse_ignored_accounts("Acme,OctoCat"),
            vec!["acme", "octocat"]
        );
        assert_eq!(parse_ignored_accounts(""), vec![""]);
    }

    #[test]
    fn installation_of_an_ignored_account_is_skipped_case_insensitively() {
        let ignored = parse_ignored_accounts("acme");
        assert_eq!(
            is_ignored_installation(&ignored, Some(&json!({"account": {"login": "ACME"}}))),
            Ok(true)
        );
        assert_eq!(
            is_ignored_installation(&ignored, Some(&json!({"account": {"login": "other"}}))),
            Ok(false)
        );
    }

    #[test]
    fn installation_without_account_fails_with_a_missing_property_error() {
        let without_account = is_ignored_installation(&[], Some(&json!({"id": 1})));
        assert_eq!(
            without_account
                .err()
                .map(|error| error.to_string())
                .as_deref(),
            Some("TypeError: Cannot read properties of undefined (reading 'login')")
        );
        let without_login = is_ignored_installation(&[], Some(&json!({"account": {}})));
        assert_eq!(
            without_login
                .err()
                .map(|error| error.to_string())
                .as_deref(),
            Some("TypeError: Cannot read properties of undefined (reading 'toLowerCase')")
        );
    }

    #[test]
    fn scheduler_listens_only_to_installation_created_and_repositories_added() {
        assert!(Scheduler::listens_to(
            "installation",
            &json!({"action": "created"})
        ));
        assert!(Scheduler::listens_to(
            "installation_repositories",
            &json!({"action": "added"})
        ));
        assert!(!Scheduler::listens_to(
            "installation",
            &json!({"action": "deleted"})
        ));
        assert!(!Scheduler::listens_to(
            "installation_repositories",
            &json!({"action": "removed"})
        ));
        assert!(!Scheduler::listens_to(
            "pull_request",
            &json!({"action": "opened"})
        ));
    }

    #[test]
    fn timer_delays_outside_the_supported_range_become_one_millisecond() {
        assert_eq!(timer_duration(0.0), Duration::from_millis(1));
        assert_eq!(timer_duration(f64::NAN), Duration::from_millis(1));
        assert_eq!(timer_duration(3_600_000.0), Duration::from_secs(3600));
        assert_eq!(timer_duration(1e12), Duration::from_millis(1));
    }
}
