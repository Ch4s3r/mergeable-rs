use crate::actions::{self, checks, comment, spawn_detached};
use crate::configuration::{self, Configuration, ErrorCode};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::filters;
use crate::interceptors;
use crate::json_value;
use crate::registry::Registry;
use crate::results::{self, Status, ValidationSummary};
use crate::settings;
use crate::validators;
use futures::future::join_all;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::Instrument;

const DOCUMENTATION_FOOTER: &str = "See the [documentation](https://github.com/mergeability/mergeable) for details on configuration.";

pub async fn execute(mut context: Context) -> Result<()> {
    let registry = Registry::default();
    interceptors::run_all(&mut context).await?;
    context.global_settings = Some(Arc::new(settings::load(&context).await));
    let configuration = configuration::load(&context).await;
    if configuration.has_errors() {
        return report_configuration_errors(&context, &configuration).await;
    }
    if context.app.behavior.log_config {
        tracing::info!(
            settings = %configuration.settings,
            "configuration.loaded"
        );
    }
    process(&context, &registry, &configuration).await
}

#[must_use]
pub fn format_error_summary(configuration: &Configuration) -> String {
    let mut summary = format!(
        "Errors were found in the configuration ({}):",
        configuration::FILE_NAME
    );
    for error in &configuration.errors {
        summary.push_str("\n- ");
        summary.push_str(&error.message);
    }
    summary.push_str("\n\n");
    summary.push_str(DOCUMENTATION_FOOTER);
    summary
}

pub async fn report_configuration_errors(
    context: &Context,
    configuration: &Configuration,
) -> Result<()> {
    let event = context.event_with_action();
    if !checks::is_supported(&event) {
        return Ok(());
    }
    let configuration_missing = configuration.has_error(ErrorCode::NoYml);
    let (conclusion, output, problem) = if configuration_missing {
        (
            "success",
            json!({
                "title": "No Config file found",
                "summary": format!("To enable Mergeable, please create a .github/mergeable.yml\n\n{DOCUMENTATION_FOOTER}")
            }),
            "missing",
        )
    } else {
        (
            "cancelled",
            json!({
                "title": "Invalid Configuration",
                "summary": format_error_summary(configuration)
            }),
            "invalid",
        )
    };
    tracing::info!(
        problem,
        errors = ?configuration.errors,
        "configuration.rejected"
    );
    let payload = json!({
        "status": "completed",
        "conclusion": conclusion,
        "output": output,
        "completed_at": checks::current_timestamp()
    });
    checks::run(context, payload).await
}

fn rule_applies_to_event(when: Option<&Value>, context: &Context) -> Result<bool> {
    let Some(Value::String(event_list)) = when else {
        return Err(Error::type_mismatch("event.split is not a function"));
    };
    let context_event = context.event_with_action();
    Ok(event_list.split(", ").any(|element| {
        let mut parts = element.split('.');
        let object = parts.next().unwrap_or_default();
        if parts.next() == Some("*") {
            object == context.event_name
        } else {
            element == context_event
        }
    }))
}

fn spawn_cancelled_check(context: &Context, title: &str, summary: &str, name: Option<String>) {
    let mut payload = json!({
        "status": "completed",
        "conclusion": "cancelled",
        "output": {"title": title, "summary": summary},
    });
    if let Some(check_name) = name {
        payload["name"] = Value::String(check_name);
    }
    payload["completed_at"] = Value::String(checks::current_timestamp());
    let task_context = context.clone();
    spawn_detached("workflow/cancelled_check", async move {
        checks::run(&task_context, payload).await
    });
}

fn rule_entries(list: Option<&Value>) -> Result<&[Value]> {
    match list {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(Error::type_mismatch(
            "arrayToIterate.forEach is not a function",
        )),
    }
}

fn entry_key(entry: &Value) -> Result<String> {
    Ok(json_value::to_display_string(json_value::read(
        Some(entry),
        "do",
    )?))
}

fn unregistered(kind: &str, key: &str) -> Error {
    Error::general(format!("no {kind} is registered under '{key}'"))
}

pub async fn filter_results(
    context: &Context,
    registry: &Registry,
    rule: &Value,
) -> Result<Vec<Value>> {
    let filter_settings = if json_value::truthy(rule.get("filter")) {
        rule_entries(rule.get("filter"))?
    } else {
        &[]
    };
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for setting in filter_settings {
        let key = entry_key(setting)?;
        let filter = registry
            .filter(&key)
            .ok_or_else(|| unregistered("filter", &key))?;
        if filters::is_event_supported(filter.as_ref(), &event_name) {
            pending.push(async move {
                filters::process_filter(filter.as_ref(), context, setting, registry).await
            });
        }
    }
    join_all(pending).await.into_iter().collect()
}

pub async fn validator_results(
    context: &Context,
    registry: &Registry,
    rule: &Value,
) -> Result<Vec<Value>> {
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for validation in rule_entries(rule.get("validate"))? {
        let key = entry_key(validation)?;
        let validator = registry
            .validator(&key)
            .ok_or_else(|| unregistered("validator", &key))?;
        if validators::is_event_supported(validator.as_ref(), &event_name) {
            pending.push(async move {
                validators::process_validate(validator.as_ref(), context, validation, registry)
                    .await
            });
        }
    }
    join_all(pending).await.into_iter().collect()
}

fn internal_error_result(error: &Error) -> Value {
    let details = format!(
        "This is a mergeable bug, please report it on our issue tracker: https://github.com/mergeability/mergeable/issues/new\n\n```\n{error}\n```\n\n"
    );
    results::consolidate_result(
        vec![results::construct_error_output(
            &json!("An error occurred"),
            json!(""),
            &json!({}),
            "Internal error!",
            Some(Value::String(details)),
        )],
        &json!({"name": "Internal error"}),
    )
}

fn action_names(rule: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let outcome_actions = Status::ALL
        .into_iter()
        .filter_map(|outcome| rule.get(outcome.as_str()).and_then(Value::as_array))
        .flatten();
    for action in outcome_actions {
        let name = json_value::to_display_string(action.get("do"));
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

async fn prepare_actions(context: &Context, registry: &Registry, rules: &[&Value]) -> Result<()> {
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for &rule in rules {
        for name in action_names(rule) {
            let action = registry
                .action(&name)
                .ok_or_else(|| unregistered("action", &name))?;
            if actions::is_event_supported(action.as_ref(), &event_name) {
                pending.push(async move {
                    actions::process_before_validate(
                        action.as_ref(),
                        context,
                        rule,
                        rule.get("name"),
                    )
                    .await
                });
            }
        }
    }
    join_all(pending).await.into_iter().collect()
}

async fn run_outcome_actions(
    context: &Context,
    registry: &Registry,
    rule: &Value,
    summary: &ValidationSummary,
) -> Result<Option<Result<()>>> {
    let Some(outcome_actions) = rule
        .get(summary.status.as_str())
        .filter(|actions| json_value::is_truthy(actions))
    else {
        return Ok(None);
    };
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for action_settings in rule_entries(Some(outcome_actions))? {
        let key = entry_key(action_settings)?;
        let action = registry
            .action(&key)
            .ok_or_else(|| unregistered("action", &key))?;
        if actions::is_event_supported(action.as_ref(), &event_name) {
            pending.push(async move {
                actions::process_after_validate(
                    action.as_ref(),
                    context,
                    action_settings,
                    rule.get("name"),
                    &summary.report,
                )
                .await
            });
        }
    }
    Ok(Some(join_all(pending).await.into_iter().collect()))
}

async fn applicable_rules<'rules>(
    context: &Context,
    registry: &Registry,
    configuration: &'rules Configuration,
) -> Result<Vec<&'rules Value>> {
    let mut applicable = Vec::new();
    for rule in rule_entries(Some(&configuration.settings))? {
        if !rule_applies_to_event(rule.get("when"), context)? {
            continue;
        }
        let filtering_span = tracing::info_span!(
            "rule.filter",
            rule.name = %checks::check_name(rule.get("name")),
        );
        match filter_results(context, registry, rule)
            .instrument(filtering_span)
            .await
        {
            Err(error) => {
                tracing::error!(
                    %error,
                    "workflow.filter.failed"
                );
                spawn_cancelled_check(
                    context,
                    "Invalid Filters",
                    &error.to_string(),
                    Some(checks::check_name(rule.get("name"))),
                );
            }
            Ok(filter_outputs) => {
                let any_failed = filter_outputs
                    .iter()
                    .any(|output| Status::of(output) == Some(Status::Fail));
                if !any_failed {
                    applicable.push(rule);
                }
            }
        }
    }
    Ok(applicable)
}

async fn evaluate_rule(
    context: &Context,
    registry: &Registry,
    rule: &Value,
    event_name: &str,
) -> Result<()> {
    let validation_results = match validator_results(context, registry, rule).await {
        Ok(outputs) => outputs,
        Err(error) => {
            tracing::error!(
                %error,
                "workflow.validator.failed"
            );
            vec![internal_error_result(&error)]
        }
    };
    let summary = results::summarize_validations(validation_results);
    tracing::Span::current().record("rule.outcome", summary.status.as_str());
    let Some(action_outcome) = run_outcome_actions(context, registry, rule, &summary).await? else {
        return Ok(());
    };
    match action_outcome {
        Err(error) => {
            tracing::error!(
                %error,
                "workflow.action.failed"
            );
            if comment::is_supported(event_name) {
                let body = format!(
                    "####  :x: Error Occurred while executing an Action \n\n If you believe this is an unexpected error, please report it on our issue tracker: https://github.com/mergeability/mergeable/issues/new \n##### Error Details \n-------------------- \n{error}"
                );
                let task_context = context.clone();
                spawn_detached("workflow/error_comment", async move {
                    comment::handle_error(&task_context, &body).await
                });
            }
        }
        Ok(()) => {
            if comment::is_supported(event_name) {
                comment::remove_error_comments(context).await?;
            }
        }
    }
    Ok(())
}

pub async fn process(
    context: &Context,
    registry: &Registry,
    configuration: &Configuration,
) -> Result<()> {
    let event_name = context.event_with_action();
    if let Err(error) = registry.register_all(&configuration.settings) {
        if checks::is_supported(&event_name) {
            spawn_cancelled_check(
                context,
                "Invalid Filters or Validators or Actions",
                &error.to_string(),
                None,
            );
        }
        return Ok(());
    }
    let rules = applicable_rules(context, registry, configuration).await?;
    prepare_actions(context, registry, &rules).await?;

    for &rule in &rules {
        let rule_span = tracing::info_span!(
            "rule",
            rule.name = %checks::check_name(rule.get("name")),
            rule.outcome = tracing::field::Empty,
        );
        evaluate_rule(context, registry, rule, &event_name)
            .instrument(rule_span)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::ConfigurationError;

    #[test]
    fn error_summary_lists_every_configuration_error() {
        let configuration = Configuration {
            errors: vec![ConfigurationError {
                code: ErrorCode::MissingMergeableNode,
                message: "The `mergeable` node is missing.".to_string(),
            }],
            settings: json!({}),
        };
        assert_eq!(
            format_error_summary(&configuration),
            "Errors were found in the configuration (.github/mergeable.yml):\n- The `mergeable` node is missing.\n\nSee the [documentation](https://github.com/mergeability/mergeable) for details on configuration."
        );
    }

    #[test]
    fn action_names_are_unique_in_outcome_order() {
        let rule = json!({"pass": [{"do": "checks"}, {"do": "comment"}], "fail": [{"do": "checks"}, {"do": "labels"}]});
        assert_eq!(action_names(&rule), vec!["checks", "comment", "labels"]);
    }
}

#[cfg(test)]
mod flex_tests {
    use super::*;
    use crate::actions::Action;
    use crate::filters::test_context::context_with_github_at;
    use crate::validators::Validator;
    use crate::yaml;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const EVERY_EVENT: &[&str] = &["*"];
    const NO_EVENT: &[&str] = &[];
    const PULL_REQUEST_AND_REVIEW_EVENTS: &[&str] = &[
        "pull_request.opened",
        "pull_request.edited",
        "pull_request_review.submitted",
    ];

    const TITLE_AND_LABEL_CONFIG: &str = "
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: wip|work in progress|do not merge
          message: 'a custom message'
      - do: label
        must_exclude:
          regex: wip|work in progress
";

    const REVIEW_MILESTONE_RULE: &str = "
  - when: pull_request_review.submitted
    validate:
      - do: milestone
        no_empty:
          enabled: true
";

    const TWO_RULES_FOR_PULL_REQUESTS_CONFIG: &str = "
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: wip|work in progress|do not merge
          message: 'a custom message'
      - do: label
        must_exclude:
          regex: wip|work in progress
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: wip|work in progress|do not merge
          message: 'a custom message'
      - do: label
        must_exclude:
          regex: wip|work in progress
";

    const PULL_REQUEST_AND_ISSUE_RULES_CONFIG: &str = "
version: 2
mergeable:
  - when: pull_request.opened
    validate:
      - do: title
        must_exclude:
          regex: 'wip'
    pass:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
    fail:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
  - when: issues.opened
    validate:
      - do: label
        must_exclude:
          regex: 'wip'
      - do: title
    pass:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
    fail:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
";

    struct CountingValidator {
        supported_events: &'static [&'static str],
        throws: bool,
        validate_calls: AtomicUsize,
        event_support_checks: AtomicUsize,
    }

    impl CountingValidator {
        fn passing_for(supported_events: &'static [&'static str]) -> Arc<CountingValidator> {
            Arc::new(CountingValidator {
                supported_events,
                throws: false,
                validate_calls: AtomicUsize::new(0),
                event_support_checks: AtomicUsize::new(0),
            })
        }

        fn throwing() -> Arc<CountingValidator> {
            Arc::new(CountingValidator {
                supported_events: EVERY_EVENT,
                throws: true,
                validate_calls: AtomicUsize::new(0),
                event_support_checks: AtomicUsize::new(0),
            })
        }

        fn validate_calls(&self) -> usize {
            self.validate_calls.load(Ordering::SeqCst)
        }

        fn event_support_checks(&self) -> usize {
            self.event_support_checks.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Validator for CountingValidator {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn supported_events(&self) -> &'static [&'static str] {
            self.event_support_checks.fetch_add(1, Ordering::SeqCst);
            self.supported_events
        }

        fn supported_settings(&self) -> Value {
            json!({})
        }

        fn checks_settings(&self) -> bool {
            false
        }

        async fn validate(
            &self,
            _context: &Context,
            _settings: &Value,
            _registry: &Registry,
        ) -> Result<Value> {
            self.validate_calls.fetch_add(1, Ordering::SeqCst);
            if self.throws {
                return Err(Error::general("Uncaught error"));
            }
            Ok(json!({"status": "pass", "name": "counting", "validations": []}))
        }
    }

    struct CountingAction {
        supported_events: &'static [&'static str],
        before_validate_calls: AtomicUsize,
        after_validate_calls: AtomicUsize,
    }

    impl CountingAction {
        fn supporting(supported_events: &'static [&'static str]) -> Arc<CountingAction> {
            Arc::new(CountingAction {
                supported_events,
                before_validate_calls: AtomicUsize::new(0),
                after_validate_calls: AtomicUsize::new(0),
            })
        }

        fn before_validate_calls(&self) -> usize {
            self.before_validate_calls.load(Ordering::SeqCst)
        }

        fn after_validate_calls(&self) -> usize {
            self.after_validate_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Action for CountingAction {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn supported_events(&self) -> &'static [&'static str] {
            self.supported_events
        }

        async fn before_validate(
            &self,
            _context: &Context,
            _rule: &Value,
            _rule_name: Option<&Value>,
        ) -> Result<()> {
            self.before_validate_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn after_validate(
            &self,
            _context: &Context,
            _settings: &Value,
            _rule_name: Option<&Value>,
            _validation_report: &Value,
        ) -> Result<()> {
            self.after_validate_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    async fn github_answering_empty_lists() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new().fallback(|uri: axum::http::Uri| async move {
            if uri.path().ends_with("/access_tokens") {
                (
                    axum::http::StatusCode::CREATED,
                    axum::Json(
                        json!({"token": "test-token", "expires_at": "2099-01-01T00:00:00Z"}),
                    ),
                )
            } else {
                (axum::http::StatusCode::OK, axum::Json(json!([])))
            }
        });
        tokio::spawn(async move { axum::serve(listener, router).await });
        format!("http://{address}")
    }

    async fn event_context(event_name: &str, action: &str) -> Context {
        context_with_github_at(
            &github_answering_empty_lists().await,
            event_name,
            json!({
                "action": action,
                "repository": {"name": "repoName", "full_name": "fullRepoName", "owner": {"login": "owner"}},
                "sender": {"login": "initiator"},
                "pull_request": {
                    "user": {"login": "creator"},
                    "title": "title",
                    "number": 1,
                    "milestone": null,
                    "base": {"repo": {"full_name": "owner/test", "private": false}, "ref": "baseRef", "sha": "sha2"},
                    "head": {"ref": "test", "sha": "sha1", "repo": {"full_name": "owner/test"}},
                },
                "issue": {"user": {"login": "creator"}, "title": "title", "number": 1, "milestone": null, "pull_request": {}},
            }),
        )
    }

    fn set_event(context: &mut Context, event_name: &str, action: &str) {
        context.event_name = event_name.to_string();
        context.payload.to_mut()["action"] = json!(action);
    }

    fn configuration(config_text: &str) -> Configuration {
        let parsed = yaml::parse(config_text).unwrap().unwrap();
        let configuration = Configuration::from_settings(parsed).unwrap();
        assert!(!configuration.has_errors(), "{:?}", configuration.errors);
        configuration
    }

    async fn run_workflow(context: &Context, registry: &Registry, config_text: &str) {
        if let Err(error) = process(context, registry, &configuration(config_text)).await {
            tracing::debug!(%error, "workflow under test failed after the observed calls");
        }
    }

    fn registry_with_checks_action() -> (Registry, Arc<CountingAction>) {
        let registry = Registry::default();
        let checks = CountingAction::supporting(PULL_REQUEST_AND_REVIEW_EVENTS);
        registry.replace_action("checks", checks.clone());
        (registry, checks)
    }

    #[tokio::test]
    async fn event_in_configuration_runs_actions_before_and_after_validation() {
        let (registry, checks) = registry_with_checks_action();
        let context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, TITLE_AND_LABEL_CONFIG).await;
        assert_eq!(checks.before_validate_calls(), 1);
        assert_eq!(checks.after_validate_calls(), 1);
    }

    #[tokio::test]
    async fn event_not_in_configuration_runs_no_actions() {
        let (registry, checks) = registry_with_checks_action();
        let context = event_context("pull_request_review", "submitted").await;
        run_workflow(&context, &registry, TITLE_AND_LABEL_CONFIG).await;
        assert_eq!(checks.before_validate_calls(), 0);
        assert_eq!(checks.after_validate_calls(), 0);
    }

    #[tokio::test]
    async fn event_matching_one_of_multiple_whens_runs_its_actions_once() {
        let (registry, checks) = registry_with_checks_action();
        let context = event_context("pull_request_review", "submitted").await;
        let config = format!("{TITLE_AND_LABEL_CONFIG}{REVIEW_MILESTONE_RULE}");
        run_workflow(&context, &registry, &config).await;
        assert_eq!(checks.before_validate_calls(), 1);
        assert_eq!(checks.after_validate_calls(), 1);
    }

    #[tokio::test]
    async fn two_whens_with_the_same_event_run_actions_once_per_rule() {
        let (registry, checks) = registry_with_checks_action();
        let context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, TWO_RULES_FOR_PULL_REQUESTS_CONFIG).await;
        assert_eq!(checks.before_validate_calls(), 2);
        assert_eq!(checks.after_validate_calls(), 2);
    }

    #[tokio::test]
    async fn two_whens_with_the_same_event_prepare_each_rules_own_actions() {
        let config = "
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: wip|work in progress|do not merge
          message: 'a custom message'
      - do: label
        must_exclude:
          regex: wip|work in progress
    fail:
      - do: assign
        assignees: ['test user']
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: wip|work in progress|do not merge
          message: 'a custom message'
      - do: label
        must_exclude:
          regex: wip|work in progress
    pass:
      - do: comment
        payload:
          body: 'test comment'
";
        let registry = Registry::default();
        let comment = CountingAction::supporting(PULL_REQUEST_AND_REVIEW_EVENTS);
        let assign = CountingAction::supporting(PULL_REQUEST_AND_REVIEW_EVENTS);
        registry.replace_action("comment", comment.clone());
        registry.replace_action("assign", assign.clone());
        let context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, config).await;
        assert_eq!(comment.before_validate_calls(), 1);
        assert_eq!(assign.before_validate_calls(), 1);
    }

    #[tokio::test]
    async fn validators_are_registered_dynamically_and_unsupported_actions_are_skipped() {
        let config = "
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_include:
          regex: wip|work in progress|do not merge
          message: 'a custom message'
      - do: label
        must_include:
          regex: wip|work in progress
    pass:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
    fail:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
";
        let registry = Registry::default();
        let context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, config).await;
        assert!(registry.validator("title").is_some());
        assert!(registry.validator("label").is_some());

        let title = CountingValidator::passing_for(EVERY_EVENT);
        let label = CountingValidator::passing_for(EVERY_EVENT);
        let checks = CountingAction::supporting(NO_EVENT);
        registry.replace_validator("title", title.clone());
        registry.replace_validator("label", label.clone());
        registry.replace_action("checks", checks.clone());
        run_workflow(&context, &registry, config).await;

        assert_eq!(title.validate_calls(), 1);
        assert_eq!(label.validate_calls(), 1);
        assert_eq!(checks.before_validate_calls(), 0);
        assert_eq!(checks.after_validate_calls(), 0);
    }

    #[tokio::test]
    async fn comma_separated_whens_run_only_event_supporting_validators_and_actions() {
        let config = "
version: 2
mergeable:
  - when: pull_request.opened, issues.opened
    validate:
      - do: title
        must_include:
          regex: wip|work in progress|do not merge
      - do: issueOnly
    pass:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
    fail:
      - do: checks
        status: success
        payload:
          title: Success!!
          summary: You are ready to merge
";
        let registry = Registry::default();
        let title = CountingValidator::passing_for(EVERY_EVENT);
        let issue_only = CountingValidator::passing_for(&["issues.opened"]);
        let checks = CountingAction::supporting(&["pull_request.opened"]);
        registry.replace_validator("title", title.clone());
        registry.replace_validator("issueOnly", issue_only.clone());
        registry.replace_action("checks", checks.clone());

        let mut context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, config).await;
        assert_eq!(title.validate_calls(), 1);
        assert_eq!(title.event_support_checks(), 1);
        assert_eq!(issue_only.validate_calls(), 0);
        assert_eq!(issue_only.event_support_checks(), 1);
        assert_eq!(checks.before_validate_calls(), 1);
        assert_eq!(checks.after_validate_calls(), 1);

        set_event(&mut context, "issues", "opened");
        run_workflow(&context, &registry, config).await;
        assert_eq!(title.validate_calls(), 2);
        assert_eq!(title.event_support_checks(), 2);
        assert_eq!(issue_only.validate_calls(), 1);
        assert_eq!(issue_only.event_support_checks(), 2);
        assert_eq!(checks.before_validate_calls(), 1);
        assert_eq!(checks.after_validate_calls(), 1);
    }

    fn registry_with_title_label_and_checks() -> (
        Registry,
        Arc<CountingValidator>,
        Arc<CountingValidator>,
        Arc<CountingAction>,
    ) {
        let registry = Registry::default();
        let title = CountingValidator::passing_for(EVERY_EVENT);
        let label = CountingValidator::passing_for(EVERY_EVENT);
        let checks = CountingAction::supporting(EVERY_EVENT);
        registry.replace_validator("title", title.clone());
        registry.replace_validator("label", label.clone());
        registry.replace_action("checks", checks.clone());
        (registry, title, label, checks)
    }

    #[tokio::test]
    async fn multiple_whens_run_only_the_rules_of_the_current_event() {
        let (registry, title, label, checks) = registry_with_title_label_and_checks();
        let mut context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, PULL_REQUEST_AND_ISSUE_RULES_CONFIG).await;
        assert_eq!(title.validate_calls(), 1);
        assert_eq!(label.validate_calls(), 0);
        assert_eq!(checks.before_validate_calls(), 1);
        assert_eq!(checks.after_validate_calls(), 1);

        set_event(&mut context, "issues", "opened");
        run_workflow(&context, &registry, PULL_REQUEST_AND_ISSUE_RULES_CONFIG).await;
        assert_eq!(title.validate_calls(), 2);
        assert_eq!(label.validate_calls(), 1);
        assert_eq!(checks.before_validate_calls(), 2);
        assert_eq!(checks.after_validate_calls(), 2);
    }

    #[tokio::test]
    async fn rules_run_only_for_the_event_in_the_context() {
        let (registry, title, label, checks) = registry_with_title_label_and_checks();
        let mut context = event_context("pull_request_review", "opened").await;
        run_workflow(&context, &registry, PULL_REQUEST_AND_ISSUE_RULES_CONFIG).await;
        assert_eq!(title.validate_calls(), 0);
        assert_eq!(label.validate_calls(), 0);
        assert_eq!(checks.before_validate_calls(), 0);
        assert_eq!(checks.after_validate_calls(), 0);

        set_event(&mut context, "pull_request", "opened");
        run_workflow(&context, &registry, PULL_REQUEST_AND_ISSUE_RULES_CONFIG).await;
        assert_eq!(title.validate_calls(), 1);
        assert_eq!(label.validate_calls(), 0);
        assert_eq!(checks.before_validate_calls(), 1);
        assert_eq!(checks.after_validate_calls(), 1);
    }

    #[tokio::test]
    async fn throwing_validator_runs_the_error_actions_but_not_the_pass_actions() {
        let config = "
version: 2
mergeable:
  - when: pull_request.opened
    validate:
      - do: error
    pass:
      - do: pass_action
    error:
      - do: error_action
";
        let registry = Registry::default();
        let error_validator = CountingValidator::throwing();
        let pass_action = CountingAction::supporting(EVERY_EVENT);
        let error_action = CountingAction::supporting(EVERY_EVENT);
        registry.replace_validator("error", error_validator.clone());
        registry.replace_action("pass_action", pass_action.clone());
        registry.replace_action("error_action", error_action.clone());

        let context = event_context("pull_request", "opened").await;
        run_workflow(&context, &registry, config).await;

        assert_eq!(error_validator.validate_calls(), 1);
        assert_eq!(error_action.before_validate_calls(), 1);
        assert_eq!(error_action.after_validate_calls(), 1);
        assert_eq!(pass_action.before_validate_calls(), 1);
        assert_eq!(pass_action.after_validate_calls(), 0);
    }
}
