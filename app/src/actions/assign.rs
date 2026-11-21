use crate::actions::Action;
use crate::annotations;
use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::Value;

pub const SUPPORTED_EVENTS: [&str; 3] = ["pull_request.*", "issues.*", "issue_comment.*"];

fn configured_assignees(settings: &Value) -> JsResult<&Vec<Value>> {
    match js::read(Some(settings), "assignees")? {
        Some(Value::Array(assignees)) => Ok(assignees),
        None => Err(JsError::reading_property_of_undefined("map")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(JsError::type_error(
            "settings.assignees.map is not a function",
        )),
    }
}

fn replace_annotations(
    assignee: &Value,
    payload: Option<&Value>,
    event: &Value,
) -> JsResult<String> {
    match assignee {
        Value::String(text) => {
            annotations::search_and_replace_special_annotations(text, payload, event)
        }
        Value::Null => Err(JsError::type_error(
            "Cannot read properties of null (reading 'replace')",
        )),
        _ => {
            js::read(js::read(payload, "user")?, "login")?;
            Err(JsError::type_error("newTemplate.replace is not a function"))
        }
    }
}

pub fn resolve_assignees(
    settings: &Value,
    payload: Option<&Value>,
    event: &Value,
) -> JsResult<Vec<String>> {
    configured_assignees(settings)?
        .iter()
        .map(|assignee| replace_annotations(assignee, payload, event))
        .collect()
}

#[derive(Default)]
pub struct Assign;

#[async_trait]
impl Action for Assign {
    fn name(&self) -> &'static str {
        "assign"
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
        let event = event_aware::get_event_attributes(context);
        let payload = event_aware::get_payload(context);
        let issue_number = js::read(payload, "number")?.cloned().unwrap_or(Value::Null);
        let assignees = resolve_assignees(settings, payload, &event)?;
        let mut checks = Vec::new();
        for assignee in assignees {
            let author_login = js::read(js::read(payload, "user")?, "login")?;
            let candidate = Value::String(assignee);
            let is_author = author_login.is_some_and(|login| js::strict_equals(&candidate, login));
            let issue_number = &issue_number;
            checks.push(async move {
                if is_author {
                    Ok(Some(candidate))
                } else {
                    api::check_user_can_be_assigned(context, issue_number, &candidate).await
                }
            });
        }
        let mut authorized = Vec::new();
        for outcome in join_all(checks).await {
            if let Some(assignee) = outcome? {
                authorized.push(assignee);
            }
        }
        api::add_assignees(context, &issue_number, &Value::Array(authorized)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> Value {
        json!({"action": "opened", "repository": {"full_name": "acme/app"}, "sender": {"login": "initiator"}})
    }

    #[test]
    fn assign_supports_pull_requests_issues_and_comments() {
        assert!(event_aware::is_event_supported(
            &SUPPORTED_EVENTS,
            "pull_request.opened"
        ));
        assert!(event_aware::is_event_supported(
            &SUPPORTED_EVENTS,
            "issues.opened"
        ));
        assert!(event_aware::is_event_supported(
            &SUPPORTED_EVENTS,
            "issue_comment.created"
        ));
        assert!(!event_aware::is_event_supported(
            &SUPPORTED_EVENTS,
            "schedule.repository"
        ));
    }

    #[test]
    fn configured_assignees_are_kept_in_order() {
        let payload = json!({"user": {"login": "creator"}});
        let assignees = resolve_assignees(
            &json!({"assignees": ["testuser1", "testuser2"]}),
            Some(&payload),
            &event(),
        )
        .unwrap();
        assert_eq!(assignees, vec!["testuser1", "testuser2"]);
    }

    #[test]
    fn author_sender_and_bot_annotations_become_logins() {
        let payload = json!({"user": {"login": "creator"}});
        let assignees = resolve_assignees(
            &json!({"assignees": ["@author", "@sender", "@bot"]}),
            Some(&payload),
            &event(),
        )
        .unwrap();
        assert_eq!(
            assignees,
            vec![
                "creator".to_string(),
                "initiator".to_string(),
                annotations::bot_name()
            ]
        );
    }

    #[test]
    fn missing_assignees_setting_fails_like_array_map_on_undefined() {
        let error = resolve_assignees(
            &json!({}),
            Some(&json!({"user": {"login": "creator"}})),
            &event(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'map')"
        );
    }
}
