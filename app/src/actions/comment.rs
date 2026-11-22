use crate::actions::{Action, actionables, spawn_detached};
use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::template;
use async_trait::async_trait;
use serde_json::Value;

pub const SUPPORTED_EVENTS: [&str; 4] = [
    "pull_request.*",
    "issues.*",
    "issue_comment.*",
    "schedule.repository",
];

#[must_use]
pub fn is_supported(event_name: &str) -> bool {
    event_aware::is_event_supported(&SUPPORTED_EVENTS, event_name)
}

fn bot_login() -> String {
    let bot_name = match std::env::var("APP_NAME") {
        Ok(name) if !name.is_empty() => name,
        _ => "Mergeable".to_string(),
    };
    format!("{}[bot]", bot_name.to_lowercase())
}

async fn fetch_comments_by_mergeable(
    context: &Context,
    issue_number: &Value,
) -> Result<Vec<Value>> {
    let comments = api::list_comments(context, issue_number).await?;
    let bot = bot_login();
    let mut by_bot = Vec::new();
    for comment in comments {
        let login = json_value::read_string(&comment, &["user", "login"])?;
        if login.to_lowercase() == bot {
            by_bot.push(comment);
        }
    }
    Ok(by_bot)
}

async fn delete_old_comments(context: &Context, old_comments: &[Value]) -> Result<()> {
    for comment in old_comments {
        let comment_id = json_value::read(Some(comment), "id")?.unwrap_or(&Value::Null);
        api::delete_comment(context, comment_id).await?;
    }
    Ok(())
}

pub async fn remove_error_comments(context: &Context) -> Result<()> {
    let Some(payload) = event_aware::payload(context) else {
        return Ok(());
    };
    let issue_number = json_value::read(Some(payload), "number")?.unwrap_or(&Value::Null);
    let old_comments = fetch_comments_by_mergeable(context, issue_number).await?;
    let mut error_comments = Vec::new();
    for comment in old_comments {
        if json_value::read_string(&comment, &["body"])?
            .to_lowercase()
            .contains("error")
        {
            error_comments.push(comment);
        }
    }
    delete_old_comments(context, &error_comments).await
}

pub async fn handle_error(context: &Context, body: &str) -> Result<()> {
    let issue_number =
        json_value::read(event_aware::payload(context), "number")?.unwrap_or(&Value::Null);
    remove_error_comments(context).await?;
    api::create_comment(context, issue_number, Value::String(body.to_string()))
        .await
        .map(|_| ())
}

#[derive(Default)]
pub struct Comment;

#[async_trait]
impl Action for Comment {
    fn name(&self) -> &'static str {
        "comment"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &SUPPORTED_EVENTS
    }

    async fn after_validate(
        &self,
        context: &Context,
        settings: &Value,
        _rule_name: Option<&Value>,
        validation_report: &Value,
    ) -> Result<()> {
        let commentables = actionables(context, validation_report);
        let event = event_aware::event_attributes(context);
        let leave_old_comment = json_value::truthy(settings.get("leave_old_comment"));
        for issue in commentables {
            let issue_number = json_value::read(Some(&*issue), "number")?
                .cloned()
                .unwrap_or(Value::Null);
            let body_template =
                json_value::read_path(settings, &["payload", "body"])?.unwrap_or(&Value::Null);
            let body = template::populate_template(
                body_template,
                validation_report,
                Some(&issue),
                &event,
            )?;
            let old_comments = if leave_old_comment {
                Ok(Vec::new())
            } else {
                fetch_comments_by_mergeable(context, &issue_number).await
            };
            let task_context = context.clone();
            spawn_detached("action/comment", async move {
                delete_old_comments(&task_context, &old_comments?).await?;
                api::create_comment(&task_context, &issue_number, Value::String(body))
                    .await
                    .map(|_| ())
            });
        }
        Ok(())
    }
}
