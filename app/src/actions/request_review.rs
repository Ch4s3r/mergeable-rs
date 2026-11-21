use crate::actions::Action;
use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use async_trait::async_trait;
use serde_json::{Value, json};

pub const SUPPORTED_EVENTS: [&str; 1] = ["pull_request.*"];

fn mapped_property(list: Option<&Value>, list_name: &str, property: &str) -> JsResult<Vec<Value>> {
    match list {
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                Ok(js::read(Some(item), property)?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect(),
        None => Err(JsError::reading_property_of_undefined("map")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(JsError::type_error(format!(
            "{list_name}.map is not a function"
        ))),
    }
}

fn configured_list(settings: &Value, key: &str) -> Value {
    settings
        .get(key)
        .filter(|value| js::is_truthy(value))
        .cloned()
        .unwrap_or_else(|| json!([]))
}

fn contains(list: &[Value], candidate: &Value) -> bool {
    list.iter().any(|item| js::strict_equals(item, candidate))
}

pub fn lodash_difference(values: &Value, excluded: &[Value]) -> Vec<Value> {
    match values {
        Value::Array(items) => items
            .iter()
            .filter(|item| !contains(excluded, item))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

pub fn lodash_intersection(values: &[Value], others: &[Value]) -> Vec<Value> {
    let mut common: Vec<Value> = Vec::new();
    for value in values {
        if contains(others, value) && !contains(&common, value) {
            common.push(value.clone());
        }
    }
    common
}

pub fn reviewers_without_author(
    reviewers: &Value,
    author_login: Option<&Value>,
) -> JsResult<Value> {
    match reviewers {
        Value::Array(items) => Ok(Value::Array(
            items
                .iter()
                .filter(|reviewer| {
                    !author_login.is_some_and(|login| js::strict_equals(reviewer, login))
                })
                .cloned()
                .collect(),
        )),
        _ => Err(JsError::type_error("reviewers.filter is not a function")),
    }
}

#[derive(Default)]
pub struct RequestReview;

#[async_trait]
impl Action for RequestReview {
    fn name(&self) -> &'static str {
        "request_review"
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
        let payload = event_aware::get_payload(context);
        let requested_reviewers = mapped_property(
            js::read(payload, "requested_reviewers")?,
            "payload.requested_reviewers",
            "login",
        )?;
        let reviewers = configured_list(settings, "reviewers");
        let author_login = match &reviewers {
            Value::Array(items) if !items.is_empty() => {
                js::read(js::read(payload, "user")?, "login")?
            }
            _ => None,
        };
        let reviewers = reviewers_without_author(&reviewers, author_login)?;
        let reviewers_to_request = lodash_difference(&reviewers, &requested_reviewers);
        let pull_number = js::read(payload, "number")?.cloned().unwrap_or(Value::Null);
        let collaborators =
            api::list_collaborators(context, context.repo_params(json!({}))?).await?;
        let collaborators_to_request = lodash_intersection(&reviewers_to_request, &collaborators);
        let requested_teams = mapped_property(
            js::read(payload, "requested_teams")?,
            "payload.requested_teams",
            "slug",
        )?;
        let teams_to_request =
            lodash_difference(&configured_list(settings, "teams"), &requested_teams);
        if collaborators_to_request.is_empty() && teams_to_request.is_empty() {
            return Ok(());
        }
        api::request_reviewers(
            context,
            &pull_number,
            Some(&Value::Array(reviewers_to_request)),
            Some(&Value::Array(teams_to_request)),
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn author_is_removed_from_reviewers_to_request() {
        let reviewers =
            reviewers_without_author(&json!(["creator", "shine2lay"]), Some(&json!("creator")))
                .unwrap();
        assert_eq!(reviewers, json!(["shine2lay"]));
    }

    #[test]
    fn already_requested_reviewers_are_not_requested_again() {
        assert!(lodash_difference(&json!(["shine2lay"]), &[json!("shine2lay")]).is_empty());
    }

    #[test]
    fn non_collaborators_are_not_counted_as_requestable() {
        assert!(lodash_intersection(&[json!("shine2lay")], &[]).is_empty());
        assert_eq!(
            lodash_intersection(&[json!("a"), json!("a"), json!("b")], &[json!("a")]),
            vec![json!("a")]
        );
    }

    #[test]
    fn already_requested_teams_are_not_requested_again() {
        assert!(lodash_difference(&json!(["developers"]), &[json!("developers")]).is_empty());
        assert_eq!(
            lodash_difference(&json!(["developers"]), &[json!("justice-league")]),
            vec![json!("developers")]
        );
    }

    #[test]
    fn non_array_teams_setting_requests_no_team() {
        assert!(lodash_difference(&json!("developers"), &[]).is_empty());
    }
}
