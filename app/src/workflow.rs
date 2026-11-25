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
        match filter_results(context, registry, rule).await {
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
        let Some(action_outcome) = run_outcome_actions(context, registry, rule, &summary).await?
        else {
            continue;
        };
        match action_outcome {
            Err(error) => {
                tracing::error!(
                    %error,
                    "workflow.action.failed"
                );
                if comment::is_supported(&event_name) {
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
                if comment::is_supported(&event_name) {
                    comment::remove_error_comments(context).await?;
                }
            }
        }
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
