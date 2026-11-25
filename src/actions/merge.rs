use crate::actions::{Action, is_comment_on_plain_issue, spawn_detached};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::template;
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use std::sync::Arc;

pub const SUPPORTED_EVENTS: [&str; 5] = [
    "pull_request.*",
    "pull_request_review.*",
    "status.*",
    "check_suite.*",
    "issue_comment.*",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Rebase,
    Squash,
}

impl MergeMethod {
    const ALL: [MergeMethod; 3] = [MergeMethod::Merge, MergeMethod::Rebase, MergeMethod::Squash];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MergeMethod::Merge => "merge",
            MergeMethod::Rebase => "rebase",
            MergeMethod::Squash => "squash",
        }
    }

    fn from_setting(configured: Option<&Value>) -> Result<Self> {
        if !json_value::truthy(configured) {
            return Ok(MergeMethod::Merge);
        }
        configured
            .and_then(Value::as_str)
            .and_then(|method| {
                MergeMethod::ALL
                    .into_iter()
                    .find(|candidate| candidate.as_str() == method)
            })
            .ok_or_else(|| {
                let supported: Vec<&str> = MergeMethod::ALL
                    .into_iter()
                    .map(MergeMethod::as_str)
                    .collect();
                Error::unsupported_setting(format!(
                    "Unknown Merge method, supported options are {}",
                    supported.join(", ")
                ))
            })
    }
}

pub fn merge_parameters(
    pull_number: &Value,
    merge_method: MergeMethod,
    commit_title: Option<&Value>,
    commit_message: Option<&Value>,
    pull_request: &Value,
) -> Result<Value> {
    let mut parameters = Map::new();
    parameters.insert("pull_number".to_string(), pull_number.clone());
    parameters.insert(
        "merge_method".to_string(),
        Value::String(merge_method.as_str().to_string()),
    );
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

struct MergeSettings {
    merge_method: MergeMethod,
    commit_title: Option<Value>,
    commit_message: Option<Value>,
}

async fn merge_unmerged_pull_request(
    context: Context,
    pull_number: Value,
    merge_settings: Arc<MergeSettings>,
) -> Result<()> {
    let pull_request = api::fetch_pull_request(&context, &pull_number).await?.data;
    let is_blocked = json_value::as_str(json_value::read(Some(&pull_request), "mergeable_state")?)
        == Some("blocked");
    let is_open =
        json_value::as_str(json_value::read(Some(&pull_request), "state")?) == Some("open");
    if is_blocked || !is_open {
        return Ok(());
    }
    let parameters = merge_parameters(
        &pull_number,
        merge_settings.merge_method,
        merge_settings.commit_title.as_ref(),
        merge_settings.commit_message.as_ref(),
        &pull_request,
    )?;
    let parameters = context.repo_params(parameters)?;
    api::merge_pull_request(&context, parameters)
        .await
        .map(|_| ())
}

fn log_detached_failure(error: &Error) {
    tracing::error!(%error, "action.merge.failed");
}

fn as_pull_request_list(value: Option<&Value>) -> Result<&[Value]> {
    match value {
        Some(Value::Array(items)) => Ok(items),
        None => Err(Error::missing_property("map")),
        Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(Error::type_mismatch(
            "relatedPullRequests.map is not a function",
        )),
    }
}

async fn related_pull_requests(context: &Context) -> Result<Vec<Cow<'_, Value>>> {
    match context.event_name.as_str() {
        "status" => {
            let commit_sha = json_value::to_display_string(context.payload.get("sha"));
            let repository = context.repository_name()?;
            let query = format!("repo:{repository} is:open {commit_sha}")
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
                    .filter(|item| json_value::truthy(item.get("pull_request")))
                    .map(Cow::Owned)
                    .collect()),
                Value::Null => Err(Error::missing_property("filter")),
                _ => Err(Error::type_mismatch("results.filter is not a function")),
            }
        }
        "check_suite" => Ok(as_pull_request_list(json_value::read(
            event_aware::payload(context),
            "pull_requests",
        )?)?
        .iter()
        .map(Cow::Borrowed)
        .collect()),
        _ => Ok(vec![
            event_aware::payload(context).map_or(Cow::Owned(Value::Null), Cow::Borrowed),
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

    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        _rule_name: Option<&Value>,
        _results: &Value,
    ) -> Result<()> {
        if is_comment_on_plain_issue(context) {
            return Ok(());
        }
        let merge_method = MergeMethod::from_setting(settings.get("merge_method"))?;
        let merge_settings = Arc::new(MergeSettings {
            merge_method,
            commit_title: settings.get("commit_title").cloned(),
            commit_message: settings.get("commit_message").cloned(),
        });
        let pull_numbers = related_pull_requests(context)
            .await?
            .iter()
            .map(|pull_request| {
                Ok(json_value::read(Some(pull_request), "number")?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect::<Result<Vec<Value>>>()?;
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
                    "action/merge",
                    merge_unmerged_pull_request(
                        context.clone(),
                        pull_number,
                        Arc::clone(&merge_settings),
                    ),
                ),
                Err(error) => log_detached_failure(&error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn pull_request() -> Value {
        json!({"mergeable_state": "clean", "state": "open", "title": "some pr", "body": "some <> ' message", "number": 10})
    }

    #[test]
    fn merge_method_defaults_to_merge() {
        assert_eq!(MergeMethod::from_setting(None).unwrap(), MergeMethod::Merge);
        assert_eq!(
            MergeMethod::from_setting(Some(&json!("squash"))).unwrap(),
            MergeMethod::Squash
        );
    }

    #[test]
    fn unknown_merge_method_is_an_unsupported_setting() {
        let error = MergeMethod::from_setting(Some(&json!("fast-forward"))).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnsupportedSetting);
        assert_eq!(
            error.message(),
            "Unknown Merge method, supported options are merge, rebase, squash"
        );
    }

    #[test]
    fn commit_title_and_message_are_omitted_when_not_configured() {
        let parameters =
            merge_parameters(&json!(1), MergeMethod::Squash, None, None, &pull_request()).unwrap();
        assert_eq!(
            parameters,
            json!({"pull_number": 1, "merge_method": "squash"})
        );
    }

    #[test]
    fn commit_title_and_message_are_rendered_from_the_pull_request_without_escaping() {
        let parameters = merge_parameters(
            &json!(10),
            MergeMethod::Squash,
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
            MergeMethod::Squash,
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
