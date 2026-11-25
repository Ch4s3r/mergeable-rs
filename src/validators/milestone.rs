use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::results::Status;
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

    fn supported_settings(&self) -> Value {
        json!({
            "no_empty": {"enabled": "boolean", "message": "string"},
            "jira": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "begins_with": {"match": ["string", "array"], "message": "string"},
            "ends_with": {"match": ["string", "array"], "message": "string"}
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let payload = event_aware::payload(context);
        let empty_title = Value::String(String::new());
        let milestone_title = match json_value::read(payload, "milestone")? {
            Some(milestone) if json_value::is_truthy(milestone) => {
                json_value::get(milestone, "title").unwrap_or(&Value::Null)
            }
            _ => &empty_title,
        };
        let mut output = validators::process_options(
            settings,
            milestone_title,
            None,
            &context.app.behavior.jira,
        )
        .await?;

        if Status::of(&output) == Some(Status::Fail) {
            let closed_issues =
                deep_validation::check_if_closes_an_issue(json_value::read(payload, "body")?);
            if !closed_issues.is_empty() {
                let milestones = deep_validation::check_if_issue_have_property(
                    context,
                    &closed_issues,
                    "milestone",
                )
                .await?;
                for issue_milestone in &milestones {
                    let issue_title =
                        json_value::get(issue_milestone, "title").unwrap_or(&Value::Null);
                    output = validators::process_options(
                        settings,
                        issue_title,
                        None,
                        &context.app.behavior.jira,
                    )
                    .await?;
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::test_context::{PullRequest, pull_request_context};

    async fn validate_with_milestone(milestone_title: Option<&str>, settings: &Value) -> Value {
        let mut context = pull_request_context(&PullRequest::default());
        context.payload.to_mut()["pull_request"]["milestone"] =
            milestone_title.map_or(Value::Null, |title| json!({"title": title}));
        validators::process_validate(&Milestone, &context, settings, &Registry::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fails_when_a_different_milestone_is_required() {
        let settings = json!({"do": "milestone", "must_include": {"regex": "Version 2"}});
        let validation = validate_with_milestone(Some("Version 1"), &settings).await;
        assert_eq!(validation["status"], "fail");
    }

    #[tokio::test]
    async fn fails_when_milestone_is_required_but_pull_request_has_none() {
        let settings = json!({"do": "milestone", "must_include": {"regex": "Version 1"}});
        let validation = validate_with_milestone(None, &settings).await;
        assert_eq!(validation["status"], "fail");
    }

    #[tokio::test]
    async fn description_names_the_missing_milestone() {
        let settings = json!({"do": "milestone", "must_include": {"regex": "Version 1"}});
        let validation = validate_with_milestone(None, &settings).await;
        assert_eq!(
            validation["validations"][0]["description"],
            "milestone does not include \"Version 1\""
        );
    }
}
