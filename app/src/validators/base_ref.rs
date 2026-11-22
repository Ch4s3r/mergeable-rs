use crate::context::Context;
use crate::error::{Error, Result};
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use crate::validators::options::lib::construct_output;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::{Map, Value, json};
use std::borrow::Cow;

pub struct BaseRef;

async fn validate_pull_requests(
    pull_requests: impl IntoIterator<Item = &Value>,
    settings: &Value,
) -> Result<Value> {
    let validator_context = json!({"name": "baseRef"});
    let base_refs: Vec<&Value> = pull_requests
        .into_iter()
        .map(|pull_request| {
            Ok(
                json_value::read(json_value::read(Some(pull_request), "base")?, "ref")?
                    .unwrap_or(&Value::Null),
            )
        })
        .collect::<Result<_>>()?;
    if base_refs.is_empty() {
        return Ok(construct_output(
            &validator_context,
            None,
            Some(settings),
            "fail",
            &Value::String("No pull requests associated with event".to_string()),
            None,
        ));
    }
    let outputs: Vec<Value> = join_all(
        base_refs
            .iter()
            .map(|base_ref| validators::process_options(settings, base_ref, None)),
    )
    .await
    .into_iter()
    .collect::<Result<_>>()?;
    Ok(results::consolidate_result(outputs, &validator_context))
}

fn array_entries<'list>(list: Option<&'list Value>, expression: &str) -> Result<&'list [Value]> {
    match list {
        Some(Value::Array(items)) => Ok(items),
        None => Err(Error::missing_property("map")),
        Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(Error::type_mismatch(format!(
            "{expression}.map is not a function"
        ))),
    }
}

async fn validate_status(
    context: &Context,
    settings: &Value,
    media_type: Option<Value>,
) -> Result<Value> {
    let owner = json_value::read_path(&context.payload, &["repository", "owner", "login"])?
        .cloned()
        .unwrap_or(Value::Null);
    let repo = json_value::read_path(&context.payload, &["repository", "name"])?
        .cloned()
        .unwrap_or(Value::Null);
    let commit_sha = context.payload.get("sha").cloned().unwrap_or(Value::Null);
    let mut request = Map::new();
    request.insert("owner".to_string(), owner);
    request.insert("repo".to_string(), repo);
    request.insert("commit_sha".to_string(), commit_sha);
    if let Some(requested_media_type) = media_type.filter(json_value::is_truthy) {
        request.insert("mediaType".to_string(), requested_media_type);
    }
    let pulls = context
        .github
        .request(
            "GET /repos/{owner}/{repo}/commits/{commit_sha}/pulls",
            Value::Object(request),
        )
        .await?;
    let Value::Array(pull_requests) = &pulls.data else {
        return Err(Error::type_mismatch("pulls.data.filter is not a function"));
    };
    let open_pull_requests = pull_requests
        .iter()
        .filter(|pull_request| json_value::as_str(pull_request.get("state")) == Some("open"));
    validate_pull_requests(open_pull_requests, settings).await
}

#[async_trait]
impl Validator for BaseRef {
    fn name(&self) -> &'static str {
        "baseRef"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[
            "pull_request.*",
            "pull_request_review.*",
            "check_suite.*",
            "status.*",
        ]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "mediaType": {"previews": "array"}
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let payload = event_aware::get_payload(context);
        let mut validation_settings = Cow::Borrowed(settings);
        let media_type = match settings.get("mediaType") {
            Some(_) => match validation_settings.to_mut() {
                Value::Object(fields) => fields.shift_remove("mediaType"),
                _ => None,
            },
            None => None,
        };
        if context.event_name == "check_suite" {
            let pull_requests = array_entries(
                json_value::read(payload, "pull_requests")?,
                "payload.pull_requests",
            )?;
            return validate_pull_requests(pull_requests, &validation_settings).await;
        }
        if context.event_name == "status" {
            return validate_status(context, &validation_settings, media_type).await;
        }
        let base_ref =
            json_value::read(json_value::read(payload, "base")?, "ref")?.unwrap_or(&Value::Null);
        validators::process_options(&validation_settings, base_ref, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_without_pull_requests_fails() {
        let settings = json!({"do": "baseRef", "must_include": {"regex": "main"}});
        let result = validate_pull_requests(&[], &settings).await.unwrap();
        assert_eq!(
            result,
            json!({"validator": {"name": "baseRef"}, "status": "fail", "description": "No pull requests associated with event", "details": {"settings": settings}})
        );
    }

    #[tokio::test]
    async fn every_pull_request_base_ref_is_validated() {
        let settings = json!({"do": "baseRef", "must_include": {"regex": "main"}});
        let pull_requests = vec![
            json!({"base": {"ref": "main"}}),
            json!({"base": {"ref": "develop"}}),
        ];
        let result = validate_pull_requests(&pull_requests, &settings)
            .await
            .unwrap();
        assert_eq!(result["status"], "fail");
        assert_eq!(result["validations"][0]["status"], "pass");
        assert_eq!(result["validations"][1]["status"], "fail");
    }
}
