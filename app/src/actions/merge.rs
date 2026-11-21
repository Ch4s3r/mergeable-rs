use crate::actions::Action;
use crate::actions::comment::spawn_detached;
use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::logging;
use crate::template;
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::{Map, Value, json};

pub const SUPPORTED_EVENTS: [&str; 5] = [
    "pull_request.*",
    "pull_request_review.*",
    "status.*",
    "check_suite.*",
    "issue_comment.*",
];
const MERGE_METHOD_OPTIONS: [&str; 3] = ["merge", "rebase", "squash"];

pub fn merge_parameters(
    pull_number: &Value,
    merge_method: &Value,
    commit_title: Option<&Value>,
    commit_message: Option<&Value>,
    pull_request: &Value,
) -> JsResult<Value> {
    let mut parameters = Map::new();
    parameters.insert("pull_number".to_string(), pull_number.clone());
    parameters.insert("merge_method".to_string(), merge_method.clone());
    if let Some(title_template) = commit_title {
        parameters.insert(
            "commit_title".to_string(),
            Value::String(template::compile_and_render(title_template, pull_request)?),
        );
    }
    if let Some(message_template) = commit_message {
        parameters.insert(
            "commit_message".to_string(),
            Value::String(template::compile_and_render(
                message_template,
                pull_request,
            )?),
        );
    }
    Ok(Value::Object(parameters))
}

async fn merge_unmerged_pull_request(
    context: Context,
    pull_number: Value,
    merge_method: Value,
    commit_title: Option<Value>,
    commit_message: Option<Value>,
) -> JsResult<()> {
    let pull_request = api::get_pull_request(&context, &pull_number).await?.data;
    let is_blocked =
        js::as_str(js::read(Some(&pull_request), "mergeable_state")?) == Some("blocked");
    let is_open = js::as_str(js::read(Some(&pull_request), "state")?) == Some("open");
    if is_blocked || !is_open {
        return Ok(());
    }
    let parameters = merge_parameters(
        &pull_number,
        &merge_method,
        commit_title.as_ref(),
        commit_message.as_ref(),
        &pull_request,
    )?;
    let parameters = context.repo_params(parameters)?;
    api::merge_pull_request(&context, parameters)
        .await
        .map(|_| ())
}

fn log_detached_failure(context: &Context, error: &JsError) {
    logging::error(
        "action/merge",
        json!({"eventId": context.event_id, "errors": error.to_string()}),
    );
}

fn validated_merge_method(settings: &Value) -> JsResult<Value> {
    let configured = settings.get("merge_method");
    if !js::truthy(configured) {
        return Ok(Value::String("merge".to_string()));
    }
    let is_supported = configured
        .and_then(Value::as_str)
        .is_some_and(|method| MERGE_METHOD_OPTIONS.contains(&method));
    if !is_supported {
        return Err(JsError::unsupported_setting(format!(
            "Unknown Merge method, supported options are {}",
            MERGE_METHOD_OPTIONS.join(", ")
        )));
    }
    Ok(configured.cloned().unwrap_or(Value::Null))
}

fn as_pull_request_list(value: Option<&Value>) -> JsResult<Vec<Value>> {
    match value {
        Some(Value::Array(items)) => Ok(items.clone()),
        None => Err(JsError::reading_property_of_undefined("map")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(JsError::type_error(
            "relatedPullRequests.map is not a function",
        )),
    }
}

async fn related_pull_requests(context: &Context) -> JsResult<Vec<Value>> {
    match context.event_name.as_str() {
        "status" => {
            let commit_sha = js::to_js_string(context.payload.get("sha"));
            let (owner, repo) = context.repo()?;
            let query = format!("repo:{owner}/{repo} is:open {commit_sha}")
                .trim()
                .to_string();
            let found = api::search_issues_and_pull_requests(
                context,
                json!({"q": query, "sort": "updated", "order": "desc", "per_page": 20}),
            )
            .await?;
            match found {
                Value::Array(items) => Ok(items
                    .into_iter()
                    .filter(|item| js::truthy(item.get("pull_request")))
                    .collect()),
                Value::Null => Err(JsError::reading_property_of_undefined("filter")),
                _ => Err(JsError::type_error("results.filter is not a function")),
            }
        }
        "check_suite" => as_pull_request_list(js::read(
            event_aware::get_payload(context),
            "pull_requests",
        )?),
        _ => Ok(vec![
            event_aware::get_payload(context)
                .cloned()
                .unwrap_or(Value::Null),
        ]),
    }
}

#[derive(Default)]
pub struct Merge;

#[async_trait]
impl Action for Merge {
    fn name(&self) -> &'static str {
        "merge"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &SUPPORTED_EVENTS
    }

    async fn before_validate(
        &self,
        _context: &Context,
        _rule: &Value,
        _rule_name: Option<&Value>,
    ) -> JsResult<()> {
        Ok(())
    }

    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        _rule_name: Option<&Value>,
        _results: &Value,
    ) -> JsResult<()> {
        if context.event_name == "issue_comment"
            && !js::truthy(js::get_path(&context.payload, &["issue", "pull_request"]))
        {
            return Ok(());
        }
        let merge_method = validated_merge_method(settings)?;
        let mut pull_numbers = Vec::new();
        for pull_request in related_pull_requests(context).await? {
            pull_numbers.push(
                js::read(Some(&pull_request), "number")?
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        let merged_checks = join_all(
            pull_numbers
                .iter()
                .map(|pull_number| api::check_if_merged(context, pull_number)),
        )
        .await;
        for (pull_number, merged_check) in pull_numbers.into_iter().zip(merged_checks) {
            match merged_check {
                Ok(true) => {}
                Ok(false) => spawn_detached(
                    context,
                    "action/merge",
                    merge_unmerged_pull_request(
                        context.clone(),
                        pull_number,
                        merge_method.clone(),
                        settings.get("commit_title").cloned(),
                        settings.get("commit_message").cloned(),
                    ),
                ),
                Err(error) => log_detached_failure(context, &error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request() -> Value {
        json!({"mergeable_state": "clean", "state": "open", "title": "some pr", "body": "some <> ' message", "number": 10})
    }

    #[test]
    fn merge_method_defaults_to_merge() {
        assert_eq!(validated_merge_method(&json!({})).unwrap(), json!("merge"));
        assert_eq!(
            validated_merge_method(&json!({"merge_method": "squash"})).unwrap(),
            json!("squash")
        );
    }

    #[test]
    fn unknown_merge_method_is_an_unsupported_setting() {
        let error = validated_merge_method(&json!({"merge_method": "fast-forward"})).unwrap_err();
        assert!(error.is_named("UnSupportedSettingError"));
        assert_eq!(
            error.message,
            "Unknown Merge method, supported options are merge, rebase, squash"
        );
    }

    #[test]
    fn commit_title_and_message_are_omitted_when_not_configured() {
        let parameters =
            merge_parameters(&json!(1), &json!("squash"), None, None, &pull_request()).unwrap();
        assert_eq!(
            parameters,
            json!({"pull_number": 1, "merge_method": "squash"})
        );
    }

    #[test]
    fn commit_title_and_message_are_rendered_from_the_pull_request_without_escaping() {
        let parameters = merge_parameters(
            &json!(10),
            &json!("squash"),
            Some(&json!("{{{ title }}} (#{{{ number }}})")),
            Some(&json!("{{{ body }}}")),
            &pull_request(),
        )
        .unwrap();
        assert_eq!(parameters["commit_title"], json!("some pr (#10)"));
        assert_eq!(parameters["commit_message"], json!("some <> ' message"));
    }

    #[test]
    fn empty_commit_templates_stay_empty() {
        let parameters = merge_parameters(
            &json!(10),
            &json!("squash"),
            Some(&json!("")),
            Some(&json!("")),
            &pull_request(),
        )
        .unwrap();
        assert_eq!(parameters["commit_title"], json!(""));
        assert_eq!(parameters["commit_message"], json!(""));
    }

    #[test]
    fn merge_supports_status_check_suite_and_review_events() {
        for event in [
            "pull_request.opened",
            "pull_request_review.submitted",
            "status.success",
            "check_suite.completed",
            "issue_comment.created",
        ] {
            assert!(
                event_aware::is_event_supported(&SUPPORTED_EVENTS, event),
                "{event}"
            );
        }
    }
}
