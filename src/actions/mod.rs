use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use async_trait::async_trait;
use serde_json::Value;
use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;
use tracing::Instrument;

pub mod assign;
pub mod checks;
pub mod close;
pub mod comment;
pub mod labels;
pub mod merge;
pub mod request_review;

#[async_trait]
pub trait Action: Send + Sync {
    fn name(&self) -> &'static str;
    fn supported_events(&self) -> &'static [&'static str];
    async fn before_validate(
        &self,
        _context: &Context,
        _rule: &Value,
        _rule_name: Option<&Value>,
    ) -> Result<()> {
        Ok(())
    }
    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        rule_name: Option<&Value>,
        validation_report: &Value,
    ) -> Result<()>;
}

#[must_use]
pub fn create(key: &str) -> Option<Arc<dyn Action>> {
    let action: Arc<dyn Action> = match key {
        "assign" => Arc::new(assign::Assign),
        "checks" => Arc::new(checks::Checks::default()),
        "close" => Arc::new(close::Close),
        "comment" => Arc::new(comment::Comment),
        "labels" => Arc::new(labels::Labels),
        "merge" => Arc::new(merge::Merge),
        "request_review" => Arc::new(request_review::RequestReview),
        _ => return None,
    };
    Some(action)
}

pub fn is_event_supported(action: &dyn Action, event_name: &str) -> bool {
    event_aware::is_event_supported(action.supported_events(), event_name)
}

pub fn spawn_detached(
    task_name: &'static str,
    task: impl Future<Output = Result<()>> + Send + 'static,
) {
    tokio::spawn(
        async move {
            if let Err(error) = task.await {
                tracing::error!(task = task_name, %error, "action.task.failed");
            }
        }
        .in_current_span(),
    );
}

pub async fn process_before_validate(
    action: &dyn Action,
    context: &Context,
    rule: &Value,
    rule_name: Option<&Value>,
) -> Result<()> {
    tracing::info!(
        action = action.name(),
        settings = %rule,
        "action.before_validate.started"
    );
    action.before_validate(context, rule, rule_name).await
}

pub async fn process_after_validate(
    action: &dyn Action,
    context: &Context,
    settings: &Value,
    rule_name: Option<&Value>,
    validation_report: &Value,
) -> Result<()> {
    tracing::info!(
        action = action.name(),
        %settings,
        "action.after_validate.started"
    );
    action
        .after_validate(context, settings, rule_name, validation_report)
        .await
}

pub fn actionables<'value>(
    context: &'value Context,
    validation_report: &'value Value,
) -> Vec<Cow<'value, Value>> {
    let schedule_results = if context.event_name == "schedule" {
        json_value::get_path(validation_report, &["validationSuites", "0", "schedule"])
            .filter(|schedule| json_value::is_truthy(schedule))
    } else {
        None
    };
    match schedule_results {
        Some(schedule) => {
            let issues = schedule
                .get("issues")
                .map_or(&[][..], json_value::as_array_or_wrap);
            json_value::concat(issues, schedule.get("pulls").unwrap_or(&json_value::NULL))
                .map(Cow::Borrowed)
                .collect()
        }
        None => {
            vec![event_aware::payload(context).map_or(Cow::Owned(Value::Null), Cow::Borrowed)]
        }
    }
}

#[must_use]
pub fn is_comment_on_plain_issue(context: &Context) -> bool {
    context.event_name == "issue_comment"
        && !json_value::truthy(json_value::get_path(
            &context.payload,
            &["issue", "pull_request"],
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppState;
    use crate::github::client::{GithubApp, GithubAppSettings};
    use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
    use rsa::{BoxedUint, PrivateKey};
    use serde_json::json;
    use std::time::Duration;

    const UNUSED_SIGNING_KEY_PRIMES: [&str; 2] = [
        "dcac7fd7aeda8f8b493e39a396e12dda08466a5d059aaab6bcc98a32a98dda1db366e705efbeca2abd0d1a588319d7c8358e77228d3b50e1a09190e618eb02b5",
        "c5bebcc34cdb61de78a8a194915a310297ac2ff838b0391b871464668fc91124284b564d404120b3e96ce14f7930b2a9a4e554659c349da3347af79c8627b3b1",
    ];
    const RSA_PUBLIC_EXPONENT: u64 = 65_537;

    fn unused_signing_key_pem() -> String {
        let [first_prime, second_prime] = UNUSED_SIGNING_KEY_PRIMES
            .map(|prime_hex| BoxedUint::from_be_slice_vartime(&hex::decode(prime_hex).unwrap()));
        PrivateKey::from_p_q(
            first_prime,
            second_prime,
            BoxedUint::from(RSA_PUBLIC_EXPONENT),
        )
        .unwrap()
        .to_pkcs1_pem(LineEnding::LF)
        .unwrap()
        .to_string()
    }

    fn context_for(event_name: &str, payload: Value) -> Context {
        let settings = GithubAppSettings {
            base_url: "http://127.0.0.1:9".to_string(),
            app_id: "1".to_string(),
            private_key_pem: unused_signing_key_pem(),
            retry_base_delay: Duration::from_millis(1),
            throttle: None,
            http_timeouts: crate::app_config::OutgoingHttpTimeouts::default(),
        };
        let app = Arc::new(GithubApp::new(settings, None).unwrap());
        Context::new(
            "upstream-test".to_string(),
            event_name.to_string(),
            payload,
            app.installation(None),
            Arc::new(AppState::default()),
        )
    }

    fn upstream_mock_payload() -> Value {
        json!({
            "sha": "sha1",
            "action": "opened",
            "repository": {"name": "repoName", "full_name": "fullRepoName", "owner": {"login": "owner"}},
            "sender": {"login": "initiator"},
            "check_suite": {"pull_requests": [{"number": 1}]},
            "pull_request": {
                "user": {"login": "creator"},
                "title": "title",
                "number": 1,
                "requested_reviewers": [],
                "requested_teams": [],
                "base": {"repo": {"full_name": "owner/test", "private": false}, "ref": "baseRef", "sha": "sha2"},
                "head": {"ref": "test", "sha": "sha1", "repo": {"full_name": "owner/test"}},
                "assignees": []
            },
            "issue": {
                "user": {"login": "creator"},
                "title": "title",
                "number": 1,
                "assignees": [],
                "pull_request": {}
            }
        })
    }

    fn scheduled_issues(numbers: &[u64]) -> Value {
        let issues: Vec<Value> = numbers
            .iter()
            .map(|number| json!({"number": number, "user": {"login": "scheduler"}}))
            .collect();
        json!({"validationSuites": [{"schedule": {"issues": issues, "pulls": []}}]})
    }

    #[test]
    fn is_event_supported_returns_correctly_with_one_supported_event() {
        let supported_events = ["issues.opened"];
        assert!(!event_aware::is_event_supported(
            &supported_events,
            "pull_request.opened"
        ));
        assert!(event_aware::is_event_supported(
            &supported_events,
            "issues.opened"
        ));
    }

    #[test]
    fn is_event_supported_returns_correctly_with_more_than_one_supported_event() {
        let supported_events = ["issues.opened", "pull_request.opened"];
        assert!(!event_aware::is_event_supported(
            &supported_events,
            "pull_request.labeled"
        ));
        assert!(event_aware::is_event_supported(
            &supported_events,
            "issues.opened"
        ));
        assert!(event_aware::is_event_supported(
            &supported_events,
            "pull_request.opened"
        ));
    }

    #[test]
    fn is_event_supported_returns_correctly_with_a_wildcard_in_the_event_name() {
        let supported_events = ["issues.opened", "pull_request.*"];
        assert!(event_aware::is_event_supported(
            &supported_events,
            "pull_request.labeled"
        ));
        assert!(!event_aware::is_event_supported(
            &supported_events,
            "issues.milestoned"
        ));
    }

    #[test]
    fn actionables_return_an_item_when_there_is_no_validation_defined() {
        let context = context_for("schedule", upstream_mock_payload());
        let report = json!({"validationSuites": []});
        assert_eq!(actionables(&context, &report).len(), 1);
    }

    #[test]
    fn actionables_return_correct_items_when_there_is_validation_and_the_event_is_schedule() {
        let context = context_for("schedule", upstream_mock_payload());
        let report_without_schedule = json!({"validationSuites": [{"status": {}}]});
        assert_eq!(actionables(&context, &report_without_schedule).len(), 1);
        let report_with_three_issues = scheduled_issues(&[1, 2, 3]);
        assert_eq!(actionables(&context, &report_with_three_issues).len(), 3);
    }

    #[test]
    fn actionables_return_the_event_item_when_event_is_not_schedule() {
        let context = context_for("pull_request", upstream_mock_payload());
        let report = json!({"validationSuites": [{"status": {}}]});
        assert_eq!(actionables(&context, &report).len(), 1);
    }

    fn assert_attributes_of_opened_event(event_name: &str) {
        let context = context_for(event_name, upstream_mock_payload());
        let attributes = event_aware::event_attributes(&context);
        assert_eq!(attributes.action, Some(&json!("opened")));
        assert_eq!(
            attributes
                .repository
                .and_then(|repository| repository.get("full_name")),
            Some(&json!("fullRepoName"))
        );
        assert_eq!(
            attributes.sender.and_then(|sender| sender.get("login")),
            Some(&json!("initiator"))
        );
    }

    #[test]
    fn event_attributes_are_extracted_from_pull_request_correctly() {
        assert_attributes_of_opened_event("pull_request");
    }

    #[test]
    fn event_attributes_are_extracted_from_issues_correctly() {
        assert_attributes_of_opened_event("issues");
    }

    #[test]
    fn event_attributes_default_on_schedule_event() {
        let context = context_for("schedule", upstream_mock_payload());
        let attributes = event_aware::event_attributes(&context);
        assert_eq!(attributes.action, Some(&json!("")));
        assert_eq!(attributes.repository, Some(&json!({})));
        assert_eq!(attributes.sender, Some(&json!({})));
    }
}
