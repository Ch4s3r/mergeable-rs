use crate::app_config::Environment;
use crate::app_state::AppState;
use crate::context::SharedPayload;
use crate::error::{Error, Result};
use crate::github::app_auth::AppJwtClient;
use crate::github::client::GithubApp;
use crate::json_value;
use crate::server;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

const MAXIMUM_TIMER_MILLISECONDS: f64 = 2_147_483_647.0;
const SCHEDULE_EVENT_NAME: &str = "schedule";
const INSTALLATIONS_ROUTE: &str = "/app/installations?per_page=100";
pub const DEFAULT_INSTALLATION_RESYNC_INTERVAL: Duration = Duration::from_secs(3600);

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
    pub installation_resync_interval: Duration,
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
            installation_resync_interval: parse_installation_resync_interval(
                environment.non_empty("MERGEABLE_INSTALLATION_RESYNC_INTERVAL"),
            ),
        }
    }
}

#[must_use]
pub fn parse_installation_resync_interval(seconds_text: Option<&str>) -> Duration {
    seconds_text
        .map(json_value::parse_decimal_number)
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .filter(|interval| !interval.is_zero())
        .unwrap_or(DEFAULT_INSTALLATION_RESYNC_INTERVAL)
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

fn installation_id_of(installation: Option<&Value>) -> Option<u64> {
    installation
        .and_then(|installation| installation.get("id"))
        .and_then(Value::as_u64)
}

#[must_use]
pub fn removed_repository_keys(payload: &Value) -> Vec<String> {
    payload
        .get("repositories_removed")
        .and_then(Value::as_array)
        .map(|repositories| repositories.iter().map(repository_key).collect())
        .unwrap_or_default()
}

struct ScheduledRepository {
    installation_id: Option<u64>,
    scheduling_sequence: u64,
    timer: JoinHandle<()>,
}

#[derive(Default)]
struct ScheduleBook {
    repositories: HashMap<String, ScheduledRepository>,
    schedulings_so_far: u64,
}

impl ScheduleBook {
    fn remove_matching(
        &mut self,
        should_remove: impl Fn(&str, &ScheduledRepository) -> bool,
    ) -> usize {
        let removed_keys: Vec<String> = self
            .repositories
            .iter()
            .filter(|(key, scheduled)| should_remove(key, scheduled))
            .map(|(key, _)| key.clone())
            .collect();
        for key in &removed_keys {
            if let Some(scheduled) = self.repositories.remove(key) {
                scheduled.timer.abort();
            }
        }
        removed_keys.len()
    }
}

#[derive(Debug, Default)]
pub struct ResyncFindings {
    pub schedulings_before_resync: u64,
    pub found_repository_keys: HashSet<String>,
    pub unverified_installation_ids: HashSet<Option<u64>>,
}

impl ResyncFindings {
    #[must_use]
    pub fn is_gone(
        &self,
        repository_key: &str,
        installation_id: Option<u64>,
        scheduling_sequence: u64,
    ) -> bool {
        scheduling_sequence < self.schedulings_before_resync
            && !self.found_repository_keys.contains(repository_key)
            && !self.unverified_installation_ids.contains(&installation_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResyncSummary {
    pub installations: usize,
    pub repositories: usize,
    pub added: usize,
    pub removed: usize,
}

pub struct Scheduler {
    github: Arc<GithubApp>,
    app: Arc<AppState>,
    app_client: Arc<AppJwtClient>,
    options: SchedulerOptions,
    schedule_book: Mutex<ScheduleBook>,
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
            schedule_book: Mutex::new(ScheduleBook::default()),
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
        let resyncing = Arc::clone(&scheduler);
        tokio::spawn(async move { resyncing.resync_periodically().await });
        scheduler
    }

    #[must_use]
    pub fn listens_to(event_name: &str, payload: &Value) -> bool {
        let action = json_value::as_str(payload.get("action"));
        matches!(
            (event_name, action),
            ("installation", Some("created" | "deleted"))
                | ("installation_repositories", Some("added" | "removed"))
        )
    }

    pub async fn handle_webhook(self: Arc<Self>, event_name: &str, payload: &Value) -> Result<()> {
        let installation = json_value::get(payload, "installation");
        match (event_name, json_value::as_str(payload.get("action"))) {
            ("installation", Some("deleted")) => {
                let installation_id = installation_id_of(installation);
                let removed = self.unschedule_installation(installation_id);
                tracing::info!(?installation_id, removed, "scheduler.installation.deleted");
                Ok(())
            }
            ("installation_repositories", Some("removed")) => {
                let removed_keys = removed_repository_keys(payload);
                let removed = self.unschedule_repositories(&removed_keys);
                tracing::info!(removed, "scheduler.repositories.removed");
                Ok(())
            }
            ("installation", _) => self.schedule_each_repository(installation).await,
            _ => {
                if self.accepts_installation(installation)? {
                    self.queue_installation(installation.cloned().unwrap_or(Value::Null));
                }
                Ok(())
            }
        }
    }

    async fn resync_periodically(&self) {
        let resync_interval = self.options.installation_resync_interval;
        let mut resync_ticks = tokio::time::interval_at(
            tokio::time::Instant::now() + resync_interval,
            resync_interval,
        );
        resync_ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            resync_ticks.tick().await;
            match self.resync().await {
                Ok(summary) => tracing::info!(
                    installations = summary.installations,
                    repositories = summary.repositories,
                    added = summary.added,
                    removed = summary.removed,
                    "scheduler.resync.completed"
                ),
                Err(error) => tracing::error!(%error, "scheduler.resync.failed"),
            }
        }
    }

    pub async fn resync(&self) -> Result<ResyncSummary> {
        let mut findings = ResyncFindings {
            schedulings_before_resync: self.locked_schedule_book().schedulings_so_far,
            ..ResyncFindings::default()
        };
        let installations = self.app_client.paginate_as_app(INSTALLATIONS_ROUTE).await?;
        let mut added = 0;
        for installation in &installations {
            let installation_id = installation_id_of(Some(installation));
            match self.accepts_installation(Some(installation)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    tracing::warn!(?installation_id, %error, "scheduler.resync.installation.unreadable");
                    findings.unverified_installation_ids.insert(installation_id);
                    continue;
                }
            }
            match self.list_repositories(installation_id).await {
                Ok(repositories) => {
                    for repository in repositories {
                        findings
                            .found_repository_keys
                            .insert(repository_key(&repository));
                        if self.schedule(installation, repository) {
                            added += 1;
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(?installation_id, %error, "scheduler.resync.repositories.failed");
                    findings.unverified_installation_ids.insert(installation_id);
                }
            }
        }
        let removed = self
            .locked_schedule_book()
            .remove_matching(|key, scheduled| {
                findings.is_gone(
                    key,
                    scheduled.installation_id,
                    scheduled.scheduling_sequence,
                )
            });
        Ok(ResyncSummary {
            installations: installations.len(),
            repositories: findings.found_repository_keys.len(),
            added,
            removed,
        })
    }

    fn locked_schedule_book(&self) -> MutexGuard<'_, ScheduleBook> {
        self.schedule_book
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn unschedule_installation(&self, installation_id: Option<u64>) -> usize {
        self.locked_schedule_book()
            .remove_matching(|_, scheduled| scheduled.installation_id == installation_id)
    }

    fn unschedule_repositories(&self, repository_keys: &[String]) -> usize {
        self.locked_schedule_book()
            .remove_matching(|key, _| repository_keys.iter().any(|removed| removed == key))
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
        for repository in self.list_repositories(installation_id).await? {
            self.schedule(installation, repository);
        }
        Ok(())
    }

    async fn list_repositories(&self, installation_id: Option<u64>) -> Result<Vec<Value>> {
        self.github
            .installation(installation_id)
            .paginate("GET /installation/repositories", json!({"per_page": 100}))
            .await
    }

    fn schedule(&self, installation: &Value, repository: Value) -> bool {
        let key = repository_key(&repository);
        let mut schedule_book = self.locked_schedule_book();
        if schedule_book.repositories.contains_key(&key) {
            return false;
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
        let scheduling_sequence = schedule_book.schedulings_so_far;
        schedule_book.schedulings_so_far += 1;
        schedule_book.repositories.insert(
            key,
            ScheduledRepository {
                installation_id: installation_id_of(Some(installation)),
                scheduling_sequence,
                timer,
            },
        );
        true
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
    fn scheduler_listens_to_installations_and_repositories_coming_and_going() {
        assert!(Scheduler::listens_to(
            "installation",
            &json!({"action": "created"})
        ));
        assert!(Scheduler::listens_to(
            "installation_repositories",
            &json!({"action": "added"})
        ));
        assert!(Scheduler::listens_to(
            "installation",
            &json!({"action": "deleted"})
        ));
        assert!(Scheduler::listens_to(
            "installation_repositories",
            &json!({"action": "removed"})
        ));
        assert!(!Scheduler::listens_to(
            "installation",
            &json!({"action": "suspend"})
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

    #[test]
    fn installation_resync_interval_defaults_to_one_hour_and_accepts_fractional_seconds() {
        assert_eq!(
            parse_installation_resync_interval(None),
            Duration::from_secs(3600)
        );
        assert_eq!(
            parse_installation_resync_interval(Some("600")),
            Duration::from_secs(600)
        );
        assert_eq!(
            parse_installation_resync_interval(Some("0.5")),
            Duration::from_millis(500)
        );
        for unusable in ["0", "-5", "soon", "Infinity"] {
            assert_eq!(
                parse_installation_resync_interval(Some(unusable)),
                DEFAULT_INSTALLATION_RESYNC_INTERVAL
            );
        }
    }

    #[test]
    fn removed_repositories_are_read_from_the_webhook_payload_by_id() {
        let payload = json!({"repositories_removed": [{"id": 7, "name": "app"}, {"id": 8}]});
        assert_eq!(removed_repository_keys(&payload), ["7", "8"]);
        assert_eq!(removed_repository_keys(&json!({})), Vec::<String>::new());
    }

    fn findings_of_a_resync(
        schedulings_before_resync: u64,
        found_repository_keys: &[&str],
        unverified_installation_ids: &[Option<u64>],
    ) -> ResyncFindings {
        ResyncFindings {
            schedulings_before_resync,
            found_repository_keys: found_repository_keys
                .iter()
                .map(|key| (*key).to_string())
                .collect(),
            unverified_installation_ids: unverified_installation_ids.iter().copied().collect(),
        }
    }

    #[test]
    fn resync_drops_a_repository_that_was_not_listed_again() {
        let findings = findings_of_a_resync(5, &["7"], &[]);
        assert!(findings.is_gone("8", Some(1), 2));
        assert!(!findings.is_gone("7", Some(1), 2));
    }

    #[test]
    fn resync_keeps_repositories_of_an_installation_whose_listing_failed() {
        let findings = findings_of_a_resync(5, &[], &[Some(1)]);
        assert!(!findings.is_gone("8", Some(1), 2));
        assert!(findings.is_gone("9", Some(2), 2));
    }

    #[test]
    fn resync_keeps_a_repository_scheduled_by_a_webhook_while_it_was_listing() {
        let findings = findings_of_a_resync(5, &[], &[]);
        assert!(findings.is_gone("8", Some(1), 4));
        assert!(!findings.is_gone("8", Some(1), 5));
    }

    #[tokio::test]
    async fn removing_scheduled_repositories_stops_their_timers() {
        let mut schedule_book = ScheduleBook::default();
        for (key, installation_id) in [("7", 1), ("8", 1), ("9", 2)] {
            schedule_book.repositories.insert(
                key.to_string(),
                ScheduledRepository {
                    installation_id: Some(installation_id),
                    scheduling_sequence: 0,
                    timer: tokio::spawn(std::future::pending()),
                },
            );
        }
        let removed =
            schedule_book.remove_matching(|_, scheduled| scheduled.installation_id == Some(1));
        assert_eq!(removed, 2);
        let remaining: Vec<&String> = schedule_book.repositories.keys().collect();
        assert_eq!(remaining, ["9"]);
    }
}
