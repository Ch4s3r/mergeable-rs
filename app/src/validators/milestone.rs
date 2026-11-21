use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::js;
use crate::registry::Registry;
use crate::validators::options_processor::deep_validation;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Milestone;

#[async_trait]
impl Validator for Milestone {
    fn name(&self) -> &'static str {
        "milestone"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*", "issues.*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "no_empty": {"enabled": "boolean", "message": "string"},
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"}
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let payload = event_aware::get_payload(context);
        let milestone_title = match js::read(payload, "milestone")? {
            Some(milestone) if js::is_truthy(milestone) => {
                js::get(milestone, "title").cloned().unwrap_or(Value::Null)
            }
            _ => Value::String(String::new()),
        };
        let mut output = validators::process_options(settings, &milestone_title, None).await?;

        if js::as_str(output.get("status")) == Some("fail") {
            let closed_issues =
                deep_validation::check_if_closes_an_issue(js::read(payload, "body")?);
            if !closed_issues.is_empty() {
                let milestones = deep_validation::check_if_issue_have_property(
                    context,
                    &closed_issues,
                    "milestone",
                )
                .await?;
                for issue_milestone in &milestones {
                    let issue_title = js::get(issue_milestone, "title")
                        .cloned()
                        .unwrap_or(Value::Null);
                    output = validators::process_options(settings, &issue_title, None).await?;
                }
            }
        }
        Ok(output)
    }
}
