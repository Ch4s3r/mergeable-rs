use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::js;
use crate::registry::Registry;
use crate::results;
use crate::validators::options::lib::construct_output;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::{Map, Value, json};

pub struct BaseRef;

async fn validate_pull_requests(pull_requests: &[Value], settings: &Value) -> JsResult<Value> {
    let validator_context = json!({"name": "baseRef"});
    let base_refs: Vec<Value> = pull_requests
        .iter()
        .map(|pull_request| {
            Ok(js::read(js::read(Some(pull_request), "base")?, "ref")?
                .cloned()
                .unwrap_or(Value::Null))
        })
        .collect::<JsResult<_>>()?;
    if base_refs.is_empty() {
        return Ok(construct_output(
            &validator_context,
            None,
            Some(settings),
            "fail",
            Value::String("No pull requests associated with event".to_string()),
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
    .collect::<JsResult<_>>()?;
    Ok(results::consolidate_result(outputs, &validator_context))
}

fn array_entries(list: Option<&Value>, expression: &str) -> JsResult<Vec<Value>> {
    match list {
        Some(Value::Array(items)) => Ok(items.clone()),
        None => Err(JsError::reading_property_of_undefined("map")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(JsError::type_error(format!(
            "{expression}.map is not a function"
        ))),
    }
}

async fn validate_status(
    context: &Context,
    settings: &Value,
    media_type: Option<Value>,
) -> JsResult<Value> {
    let owner = js::read_path(&context.payload, &["repository", "owner", "login"])?
        .cloned()
        .unwrap_or(Value::Null);
    let repo = js::read_path(&context.payload, &["repository", "name"])?
        .cloned()
        .unwrap_or(Value::Null);
    let commit_sha = context.payload.get("sha").cloned().unwrap_or(Value::Null);
    let mut request = Map::new();
    request.insert("owner".to_string(), owner);
    request.insert("repo".to_string(), repo);
    request.insert("commit_sha".to_string(), commit_sha);
    if let Some(requested_media_type) = media_type.filter(js::is_truthy) {
        request.insert("mediaType".to_string(), requested_media_type);
    }
    let pulls = context
        .github
        .request(
            "GET /repos/{owner}/{repo}/commits/{commit_sha}/pulls",
            Value::Object(request),
        )
        .await?;
    let open_pull_requests: Vec<Value> = match &pulls.data {
        Value::Array(pull_requests) => pull_requests
            .iter()
            .filter(|pull_request| js::as_str(pull_request.get("state")) == Some("open"))
            .cloned()
            .collect(),
        _ => return Err(JsError::type_error("pulls.data.filter is not a function")),
    };
    validate_pull_requests(&open_pull_requests, settings).await
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

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "mediaType": {"previews": "array"}
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let payload = event_aware::get_payload(context);
        let media_type = settings.get("mediaType").cloned();
        let mut validation_settings = settings.clone();
        if let Value::Object(fields) = &mut validation_settings {
            fields.shift_remove("mediaType");
        }
        if context.event_name == "check_suite" {
            let pull_requests =
                array_entries(js::read(payload, "pull_requests")?, "payload.pull_requests")?;
            return validate_pull_requests(&pull_requests, &validation_settings).await;
        }
        if context.event_name == "status" {
            return validate_status(context, &validation_settings, media_type).await;
        }
        let base_ref = js::read(js::read(payload, "base")?, "ref")?
            .cloned()
            .unwrap_or(Value::Null);
        validators::process_options(&validation_settings, &base_ref, None).await
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
