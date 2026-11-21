use crate::configuration;
use crate::context::Context;
use crate::error::JsResult;
use crate::flex;
use crate::github::api;
use crate::js;
use crate::logging;
use crate::metadata;
use crate::registry::Registry;
use futures::future::join_all;
use serde_json::{Value, json};

pub async fn run_all(context: &mut Context) -> JsResult<()> {
    check_rerun(context).await?;
    milestoned(context).await?;
    push(context).await
}

async fn check_rerun(context: &mut Context) -> JsResult<()> {
    if !(context.event_name == "check_run" && js::as_str(context.action()) == Some("rerequested")) {
        return Ok(());
    }
    let Some(check_run) = context
        .payload
        .get("check_run")
        .filter(|check_run| js::is_truthy(check_run))
        .cloned()
    else {
        return Ok(());
    };
    if !js::truthy(check_run.get("output")) {
        return Ok(());
    }
    let text = match js::read_path(&check_run, &["output", "text"])? {
        Some(Value::String(text)) => text.clone(),
        Some(other) => js::to_js_string(Some(other)),
        None => {
            return Err(crate::error::JsError::reading_property_of_undefined(
                "indexOf",
            ));
        }
    };
    let meta = metadata::deserialize(&text)?;
    let check_run_id = check_run.get("id").cloned().unwrap_or(Value::Null);
    let meta_id = meta.get("id").cloned().unwrap_or(Value::Null);
    if !js::strict_equals(&check_run_id, &meta_id) {
        logging::warn("interceptors/checkReRun", || {
            json!({
                "logType": "potential_injection",
                "message": "ids in payload do not match. Potential injection.",
                "checkRun": check_run,
                "meta": meta,
            })
        });
        return Ok(());
    }
    let pull_requests = js::read(Some(&check_run), "pull_requests")?
        .cloned()
        .unwrap_or(Value::Null);
    let first_pull_request = match &pull_requests {
        Value::Array(items) if items.is_empty() => return Ok(()),
        Value::Array(items) => items[0].clone(),
        _ => {
            return Err(crate::error::JsError::reading_property_of_undefined(
                "length",
            ));
        }
    };
    let pull_number = js::read(Some(&first_pull_request), "number")?
        .cloned()
        .unwrap_or(Value::Null);
    let pull_request = api::get_pull_request(context, &pull_number).await?;
    if let Value::Object(payload) = &mut context.payload {
        match meta.get("action") {
            Some(action) => payload.insert("action".to_string(), action.clone()),
            None => payload.shift_remove("action"),
        };
        payload.insert("pull_request".to_string(), pull_request.data);
    }
    context.event_name = js::to_js_string(meta.get("eventName"));
    Ok(())
}

async fn milestoned(context: &mut Context) -> JsResult<()> {
    let is_milestone_action = matches!(
        js::as_str(context.action()),
        Some("milestoned") | Some("demilestoned")
    );
    if !(context.event_name == "issues" && is_milestone_action) {
        return Ok(());
    }
    if !js::truthy(js::read_path(&context.payload, &["issue", "pull_request"])?) {
        return Ok(());
    }
    let issue_number = js::read_path(&context.payload, &["issue", "number"])?
        .cloned()
        .unwrap_or(Value::Null);
    let response = api::get_pull_request(context, &issue_number).await?;
    let mut pull_request = response.data;
    if let Value::Object(fields) = &mut pull_request {
        fields.insert(
            "action".to_string(),
            context.action().cloned().unwrap_or(Value::Null),
        );
    }
    context.event_name = "pull_request".to_string();
    if let Value::Object(payload) = &mut context.payload {
        payload.insert("pull_request".to_string(), pull_request);
    }
    Ok(())
}

async fn push(context: &mut Context) -> JsResult<()> {
    if context.event_name != "push" {
        return Ok(());
    }
    let Some(head_commit) = context
        .payload
        .get("head_commit")
        .filter(|commit| js::is_truthy(commit))
        .cloned()
    else {
        return Ok(());
    };
    let config_path = match std::env::var("CONFIG_PATH") {
        Ok(path) if !path.is_empty() => path,
        _ => "mergeable.yml".to_string(),
    };
    let config_file = Value::String(format!(".github/{config_path}"));
    let changed = js::includes(head_commit.get("added"), &config_file)?
        || js::includes(head_commit.get("modified"), &config_file)?;
    if !changed {
        return Ok(());
    }
    let configuration = configuration::instance_with_context(context).await;
    if configuration.has_errors() {
        flex::log_and_process_config_errors(context, &configuration).await?;
        return Ok(());
    }
    let registry = Registry::default();
    let response = api::list_pull_requests(context).await?;
    let pulls = match response.data {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let pending = pulls.into_iter().map(|pull_request| {
        let mut rerun_context = context.clone();
        rerun_context.event_name = "pull_request".to_string();
        if let Value::Object(payload) = &mut rerun_context.payload {
            payload.insert("pull_request".to_string(), pull_request);
            payload.insert(
                "action".to_string(),
                Value::String("push_synchronize".to_string()),
            );
        }
        let registry = &registry;
        let configuration = &configuration;
        async move { flex::process_workflow(&rerun_context, registry, configuration).await }
    });
    join_all(pending)
        .await
        .into_iter()
        .collect::<JsResult<Vec<()>>>()
        .map(|_| ())
}
