use crate::error::{JsError, JsResult};
use crate::github::app_auth::AppJwtClient;
use crate::github::client::GithubApp;
use crate::js;
use crate::logging;
use crate::server;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const NODE_TIMEOUT_MAX_MILLISECONDS: f64 = 2_147_483_647.0;
const SCHEDULE_EVENT_NAME: &str = "schedule";

pub struct SchedulerOptions {
    pub delay: bool,
    pub interval_milliseconds: f64,
    pub ignored_accounts: Vec<String>,
}

impl SchedulerOptions {
    pub fn from_env(interval_milliseconds: f64) -> Self {
        let environment_text = |name: &str| std::env::var(name).unwrap_or_default();
        SchedulerOptions {
            delay: environment_text("DISABLE_DELAY").is_empty(),
            interval_milliseconds,
            ignored_accounts: parse_ignored_accounts(&environment_text("IGNORED_ACCOUNTS")),
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

pub fn js_number(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    match trimmed {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if trimmed.chars().any(|character| {
            character.is_ascii_alphabetic() && !matches!(character, 'e' | 'E')
        }) =>
        {
            f64::NAN
        }
        _ => trimmed.parse::<f64>().unwrap_or(f64::NAN),
    }
}

pub fn node_timer_duration(milliseconds: f64) -> Duration {
    let effective = if (1.0..=NODE_TIMEOUT_MAX_MILLISECONDS).contains(&milliseconds) {
        milliseconds
    } else {
        1.0
    };
    Duration::from_secs_f64(effective / 1000.0)
}

pub fn scheduled_event_payload(installation: &Value, repository: &Value) -> Value {
    json!({
        "action": "repository",
        "installation": installation,
        "repository": repository,
    })
}

pub fn is_ignored_installation(
    ignored_accounts: &[String],
    installation: Option<&Value>,
) -> JsResult<bool> {
    let account = js::read(installation, "account")?;
    let login = js::read(account, "login")?;
    let lowercase_login = match login {
        Some(Value::String(text)) => text.to_lowercase(),
        Some(Value::Null) | None => {
            js::read(login, "toLowerCase")?;
            String::new()
        }
        Some(_) => {
            return Err(JsError::type_error(
                "installation.account.login.toLowerCase is not a function",
            ));
        }
    };
    Ok(ignored_accounts.contains(&lowercase_login))
}

fn repository_key(repository: &Value) -> String {
    js::to_js_string(js::get(repository, "id"))
}

pub struct Scheduler {
    github: Arc<GithubApp>,
    app_client: Arc<AppJwtClient>,
    options: SchedulerOptions,
    intervals: Mutex<HashMap<String, JoinHandle<()>>>,
    installation_queue: mpsc::UnboundedSender<Value>,
}

impl Scheduler {
    pub fn start(
        github: Arc<GithubApp>,
        app_client: Arc<AppJwtClient>,
        options: SchedulerOptions,
    ) -> Arc<Scheduler> {
        let (installation_queue, mut queued_installations) = mpsc::unbounded_channel::<Value>();
        let scheduler = Arc::new(Scheduler {
            github,
            app_client,
            options,
            intervals: Mutex::new(HashMap::new()),
            installation_queue,
        });
        let worker = Arc::clone(&scheduler);
        tokio::spawn(async move {
            while let Some(installation) = queued_installations.recv().await {
                if let Err(error) = worker.schedule_each_repository(Some(&installation)).await {
                    logging::error("scheduler", || json!({"message": error.to_string()}));
                }
            }
        });
        let setup = Arc::clone(&scheduler);
        tokio::spawn(async move {
            if let Err(error) = setup.setup().await {
                logging::error("scheduler", || json!({"message": error.to_string()}));
            }
        });
        scheduler
    }

    pub fn listens_to(event_name: &str, payload: &Value) -> bool {
        let action = js::as_str(payload.get("action"));
        matches!(
            (event_name, action),
            ("installation", Some("created")) | ("installation_repositories", Some("added"))
        )
    }

    pub async fn handle_webhook(
        self: Arc<Self>,
        event_name: &str,
        payload: &Value,
    ) -> JsResult<()> {
        let installation = js::get(payload, "installation");
        match event_name {
            "installation" => self.schedule_each_repository(installation).await,
            _ => self.setup_installation(installation),
        }
    }

    async fn setup(&self) -> JsResult<()> {
        let installations = self
            .app_client
            .paginate_as_app("/app/installations?per_page=100")
            .await?;
        for installation in installations {
            self.setup_installation(Some(&installation))?;
        }
        Ok(())
    }

    fn setup_installation(&self, installation: Option<&Value>) -> JsResult<()> {
        if is_ignored_installation(&self.options.ignored_accounts, installation)? {
            logging::info(
                "scheduler",
                || json!({"installation": installation, "msg": "Installation is ignored"}),
            );
            return Ok(());
        }
        let queued_installation = installation.cloned().unwrap_or(Value::Null);
        if self.installation_queue.send(queued_installation).is_err() {
            logging::error("scheduler", || json!({"message": "scheduler queue closed"}));
        }
        Ok(())
    }

    async fn schedule_each_repository(&self, installation: Option<&Value>) -> JsResult<()> {
        let installation_id = js::read(installation, "id")?.and_then(Value::as_u64);
        let installation = installation.cloned().unwrap_or(Value::Null);
        let client = self.github.installation(installation_id);
        let repositories = client
            .paginate("GET /installation/repositories", json!({"per_page": 100}))
            .await?;
        for repository in repositories {
            self.schedule(&installation, &repository);
        }
        Ok(())
    }

    fn schedule(&self, installation: &Value, repository: &Value) {
        let key = repository_key(repository);
        let Ok(mut intervals) = self.intervals.lock() else {
            return;
        };
        if intervals.contains_key(&key) {
            return;
        }
        let interval_milliseconds = self.options.interval_milliseconds;
        let delay_milliseconds = if self.options.delay {
            interval_milliseconds * rand::random::<f64>()
        } else {
            0.0
        };
        logging::debug(
            "scheduler",
            || json!({"repository": repository, "delay": delay_milliseconds, "interval": interval_milliseconds, "msg": "Scheduling interval"}),
        );
        let payload = scheduled_event_payload(installation, repository);
        let github = Arc::clone(&self.github);
        let timer = tokio::spawn(async move {
            tokio::time::sleep(node_timer_duration(delay_milliseconds)).await;
            loop {
                let receive_github = Arc::clone(&github);
                let receive_payload = payload.clone();
                tokio::spawn(async move {
                    let outcome = server::run_mergeable(
                        receive_github,
                        String::new(),
                        SCHEDULE_EVENT_NAME.to_string(),
                        receive_payload,
                    )
                    .await;
                    if let Err(error) = outcome {
                        logging::error("scheduler", || json!({"message": error.to_string()}));
                    }
                });
                tokio::time::sleep(node_timer_duration(interval_milliseconds)).await;
            }
        });
        intervals.insert(key, timer);
    }

    pub fn stop(&self, repository: &Value) {
        logging::info(
            "scheduler",
            || json!({"repository": repository, "msg": "Canceling interval"}),
        );
        if let Ok(intervals) = self.intervals.lock()
            && let Some(timer) = intervals.get(&repository_key(repository))
        {
            timer.abort();
        }
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
            scheduled_event_payload(&installation, &repository),
            json!({"action": "repository", "installation": installation, "repository": repository})
        );
        let payload = scheduled_event_payload(&installation, &repository);
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
    fn installation_without_account_fails_like_javascript() {
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
    fn timer_delays_outside_the_node_range_become_one_millisecond() {
        assert_eq!(node_timer_duration(0.0), Duration::from_millis(1));
        assert_eq!(node_timer_duration(f64::NAN), Duration::from_millis(1));
        assert_eq!(node_timer_duration(3_600_000.0), Duration::from_secs(3600));
        assert_eq!(node_timer_duration(1e12), Duration::from_millis(1));
    }

    #[test]
    fn interval_text_is_converted_like_javascript_number() {
        assert_eq!(js_number("3600"), 3600.0);
        assert_eq!(js_number(" 0.5 "), 0.5);
        assert_eq!(js_number(""), 0.0);
        assert!(js_number("abc").is_nan());
        assert!(js_number("inf").is_nan());
    }
}
