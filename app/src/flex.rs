use crate::actions::{self, checks, comment};
use crate::configuration::{self, Configuration, error_codes};
use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::filters;
use crate::interceptors;
use crate::js;
use crate::logging;
use crate::registry::Registry;
use crate::results;
use crate::settings;
use crate::validators;
use futures::future::join_all;
use serde_json::{Value, json};

pub async fn execute_mergeable(mut context: Context) -> JsResult<()> {
    let registry = Registry::default();
    interceptors::run_all(&mut context).await?;
    context.global_settings = Some(settings::instance_with_context(&context).await);
    let configuration = configuration::instance_with_context(&context).await;
    if configuration.has_errors() {
        return log_and_process_config_errors(&context, &configuration).await;
    }
    if std::env::var("LOG_CONFIG").is_ok_and(|value| !value.is_empty()) {
        logging::info("flex", || {
            json!({
                "logType": "config",
                "eventId": context.event_id,
                "repo": context.repository_full_name(),
                "settings": js::stringify(&configuration.settings),
            })
        });
    }
    process_workflow(&context, &registry, &configuration).await
}

pub fn format_error_summary(configuration: &Configuration) -> String {
    let mut summary = format!(
        "Errors were found in the configuration ({}):",
        configuration::FILE_NAME
    );
    for (_, message) in &configuration.errors {
        summary.push_str("\n- ");
        summary.push_str(message);
    }
    summary.push_str("\n\nSee the [documentation](https://github.com/mergeability/mergeable) for details on configuration.");
    summary
}

pub async fn log_and_process_config_errors(
    context: &Context,
    configuration: &Configuration,
) -> JsResult<()> {
    let event = context.event_with_action();
    if !checks::is_supported(&event) {
        return Ok(());
    }
    let mut payload = json!({
        "status": "completed",
        "conclusion": "cancelled",
        "output": {
            "title": "Invalid Configuration",
            "summary": format_error_summary(configuration)
        },
        "completed_at": checks::now_iso()
    });
    let mut log_type = "config_invalid_yml";
    if configuration.has_error(error_codes::NO_YML) {
        payload["conclusion"] = json!("success");
        payload["output"] = json!({
            "title": "No Config file found",
            "summary": "To enable Mergeable, please create a .github/mergeable.yml\n\nSee the [documentation](https://github.com/mergeability/mergeable) for details on configuration."
        });
        log_type = "config_no_yml";
    }
    logging::info("flex", || {
        json!({
            "logType": log_type,
            "eventId": context.event_id,
            "repo": context.repository_full_name(),
            "event": event,
            "errors": configuration.errors.iter().map(|(code, message)| json!([code, message])).collect::<Vec<_>>(),
        })
    });
    checks::run(context, &payload).await
}

fn is_event_in_context(when: Option<&Value>, context: &Context) -> JsResult<bool> {
    let Some(Value::String(event_list)) = when else {
        return Err(JsError::type_error("event.split is not a function"));
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

fn spawn_cancelled_check(context: &Context, title: &str, summary: String, name: Option<String>) {
    let mut payload = json!({
        "status": "completed",
        "conclusion": "cancelled",
        "output": {"title": title, "summary": summary},
    });
    if let Some(check_name) = name {
        payload["name"] = Value::String(check_name);
    }
    payload["completed_at"] = Value::String(checks::now_iso());
    let task_context = context.clone();
    comment::spawn_detached(context, "flex", async move {
        checks::run(&task_context, &payload).await
    });
}

fn entries(list: Option<&Value>) -> JsResult<Vec<Value>> {
    match list {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => Ok(items.clone()),
        Some(_) => Err(JsError::type_error(
            "arrayToIterate.forEach is not a function",
        )),
    }
}

async fn run_filters(
    context: &Context,
    registry: &Registry,
    filter_settings: &[Value],
) -> JsResult<Vec<Value>> {
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for setting in filter_settings {
        let key = js::to_js_string(js::read(Some(setting), "do")?);
        let filter = registry
            .filter(&key)
            .ok_or_else(|| JsError::reading_property_of_undefined("isEventSupported"))?;
        if filters::is_event_supported(filter.as_ref(), &event_name) {
            pending.push(async move {
                filters::process_filter(filter.as_ref(), context, setting, registry).await
            });
        }
    }
    join_all(pending).await.into_iter().collect()
}

pub async fn filter_results(
    context: &Context,
    registry: &Registry,
    rule: &Value,
) -> JsResult<Vec<Value>> {
    let filter_settings = if js::truthy(rule.get("filter")) {
        entries(rule.get("filter"))?
    } else {
        Vec::new()
    };
    run_filters(context, registry, &filter_settings).await
}

pub async fn validator_results(
    context: &Context,
    registry: &Registry,
    rule: &Value,
) -> JsResult<Vec<Value>> {
    let event_name = context.event_with_action();
    let validations = entries(rule.get("validate"))?;
    let mut pending = Vec::new();
    for validation in &validations {
        let key = js::to_js_string(js::read(Some(validation), "do")?);
        let validator = registry
            .validator(&key)
            .ok_or_else(|| JsError::reading_property_of_undefined("isEventSupported"))?;
        if validators::is_event_supported(validator.as_ref(), &event_name) {
            pending.push(async move {
                validators::process_validate(validator.as_ref(), context, validation, registry)
                    .await
            });
        }
    }
    join_all(pending).await.into_iter().collect()
}

fn internal_error_result(error: &JsError) -> Value {
    let details = format!(
        "This is a mergeable bug, please report it on our issue tracker: https://github.com/mergeability/mergeable/issues/new\n\n```\n{error}\n```\n\n"
    );
    results::consolidate_result(
        vec![results::construct_error_output(
            &json!("An error occurred"),
            &json!(""),
            &json!({}),
            "Internal error!",
            Some(Value::String(details)),
        )],
        &json!({"name": "Internal error"}),
    )
}

fn action_names(rule: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for outcome in ["pass", "fail", "error"] {
        if let Some(Value::Array(actions)) = rule.get(outcome) {
            for action in actions {
                let name = js::to_js_string(action.get("do"));
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

async fn process_pre_actions(
    context: &Context,
    registry: &Registry,
    rules: &[Value],
) -> JsResult<()> {
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for rule in rules {
        for name in action_names(rule) {
            let action = registry
                .action(&name)
                .ok_or_else(|| JsError::reading_property_of_undefined("isEventSupported"))?;
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
    join_all(pending)
        .await
        .into_iter()
        .collect::<JsResult<Vec<()>>>()
        .map(|_| ())
}

async fn run_actions(
    context: &Context,
    registry: &Registry,
    rule: &Value,
    stats: &Value,
) -> JsResult<Option<JsResult<()>>> {
    let status = js::to_js_string(stats.get("validationStatus"));
    let Some(outcome_actions) = rule.get(&status).filter(|actions| js::is_truthy(actions)) else {
        return Ok(None);
    };
    let event_name = context.event_with_action();
    let mut pending = Vec::new();
    for action_settings in entries(Some(outcome_actions))? {
        let key = js::to_js_string(js::read(Some(&action_settings), "do")?);
        let action = registry
            .action(&key)
            .ok_or_else(|| JsError::reading_property_of_undefined("isEventSupported"))?;
        if actions::is_event_supported(action.as_ref(), &event_name) {
            pending.push(async move {
                actions::process_after_validate(
                    action.as_ref(),
                    context,
                    &action_settings,
                    rule.get("name"),
                    stats,
                )
                .await
            });
        }
    }
    let outcome = join_all(pending)
        .await
        .into_iter()
        .collect::<JsResult<Vec<()>>>()
        .map(|_| ());
    Ok(Some(outcome))
}

pub async fn process_workflow(
    context: &Context,
    registry: &Registry,
    configuration: &Configuration,
) -> JsResult<()> {
    let event_name = context.event_with_action();
    if let Err(error) = registry.register_all(&configuration.settings) {
        if checks::is_supported(&event_name) {
            spawn_cancelled_check(
                context,
                "Invalid Filters or Validators or Actions",
                error.to_string(),
                None,
            );
        }
        return Ok(());
    }
    let mut applicable_rules = Vec::new();
    for rule in entries(Some(&configuration.settings))? {
        if !is_event_in_context(rule.get("when"), context)? {
            continue;
        }
        match filter_results(context, registry, &rule).await {
            Err(error) => {
                logging::error("flex", || {
                    json!({
                        "logType": "unknown_error_filter",
                        "eventId": context.event_id,
                        "errors": error.to_string(),
                        "repo": context.repository_full_name(),
                        "event": event_name,
                    })
                });
                spawn_cancelled_check(
                    context,
                    "Invalid Filters",
                    error.to_string(),
                    Some(checks::create_check_name(rule.get("name"))),
                );
                continue;
            }
            Ok(filter_outputs) => {
                let any_failed = filter_outputs
                    .iter()
                    .any(|output| output.get("status").and_then(Value::as_str) == Some("fail"));
                if any_failed {
                    continue;
                }
            }
        }
        applicable_rules.push(rule);
    }

    process_pre_actions(context, registry, &applicable_rules).await?;

    for rule in &applicable_rules {
        let validation_results = match validator_results(context, registry, rule).await {
            Ok(outputs) => outputs,
            Err(error) => {
                logging::error("flex", || {
                    json!({
                        "logType": "unknown_error_validator",
                        "eventId": context.event_id,
                        "errors": error.to_string(),
                        "repo": context.repository_full_name(),
                        "event": event_name,
                    })
                });
                vec![internal_error_result(&error)]
            }
        };
        let stats = results::extract_validation_stats(&validation_results);
        let Some(action_outcome) = run_actions(context, registry, rule, &stats).await? else {
            continue;
        };
        match action_outcome {
            Err(error) => {
                let body = format!(
                    "####  :x: Error Occurred while executing an Action \n\n If you believe this is an unexpected error, please report it on our issue tracker: https://github.com/mergeability/mergeable/issues/new \n##### Error Details \n-------------------- \n{error}"
                );
                logging::error("flex", || {
                    json!({
                        "logType": "unknown_error_action",
                        "eventId": context.event_id,
                        "errors": error.to_string(),
                        "repo": context.repository_full_name(),
                        "event": event_name,
                    })
                });
                if comment::is_supported(&event_name) {
                    let task_context = context.clone();
                    comment::spawn_detached(context, "flex", async move {
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

    #[test]
    fn error_summary_lists_every_configuration_error() {
        let configuration = Configuration {
            errors: vec![(20, "The `mergeable` node is missing.".to_string())],
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
