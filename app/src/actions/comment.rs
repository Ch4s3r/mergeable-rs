use crate::actions::{Action, get_actionables};
use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::template;
use async_trait::async_trait;
use serde_json::Value;

pub const SUPPORTED_EVENTS: [&str; 4] = [
    "pull_request.*",
    "issues.*",
    "issue_comment.*",
    "schedule.repository",
];

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
) -> JsResult<Vec<Value>> {
    let comments = api::list_comments(context, issue_number).await?;
    let bot = bot_login();
    let mut by_bot = Vec::new();
    for comment in comments {
        let login = js::read_string(&comment, &["user", "login"])?;
        if login.to_lowercase() == bot {
            by_bot.push(comment);
        }
    }
    Ok(by_bot)
}

async fn delete_old_comments(context: &Context, old_comments: &[Value]) -> JsResult<()> {
    for comment in old_comments {
        let comment_id = js::read(Some(comment), "id")?
            .cloned()
            .unwrap_or(Value::Null);
        api::delete_comment(context, &comment_id).await?;
    }
    Ok(())
}

pub async fn remove_error_comments(context: &Context) -> JsResult<()> {
    let Some(payload) = event_aware::get_payload(context) else {
        return Ok(());
    };
    let issue_number = js::read(Some(payload), "number")?
        .cloned()
        .unwrap_or(Value::Null);
    let old_comments = fetch_comments_by_mergeable(context, &issue_number).await?;
    let mut error_comments = Vec::new();
    for comment in old_comments {
        if js::read_string(&comment, &["body"])?
            .to_lowercase()
            .contains("error")
        {
            error_comments.push(comment);
        }
    }
    delete_old_comments(context, &error_comments).await
}

pub async fn handle_error(context: &Context, body: &str) -> JsResult<()> {
    let issue_number = js::read(event_aware::get_payload(context), "number")?
        .cloned()
        .unwrap_or(Value::Null);
    remove_error_comments(context).await?;
    api::create_comment(context, &issue_number, &Value::String(body.to_string()))
        .await
        .map(|_| ())
}

pub fn spawn_detached(
    context: &Context,
    task_name: &'static str,
    future: impl std::future::Future<Output = JsResult<()>> + Send + 'static,
) {
    let event_id = context.event_id.clone();
    tokio::spawn(async move {
        if let Err(error) = future.await {
            tracing::error!(task = task_name, event_id = %event_id, %error, "comment action failed");
        }
    });
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
        results: &Value,
    ) -> JsResult<()> {
        let commentables = get_actionables(context, results);
        let event = event_aware::get_event_attributes(context);
        let leave_old_comment = js::truthy(settings.get("leave_old_comment"));
        for issue in commentables {
            let issue_number = js::read(Some(&*issue), "number")?
                .cloned()
                .unwrap_or(Value::Null);
            let body_template =
                js::read_path(settings, &["payload", "body"])?.unwrap_or(&Value::Null);
            let body = template::populate_template(body_template, results, Some(&issue), &event)?;
            let old_comments = if leave_old_comment {
                Ok(Vec::new())
            } else {
                fetch_comments_by_mergeable(context, &issue_number).await
            };
            let task_context = context.clone();
            spawn_detached(context, "action/comment", async move {
                delete_old_comments(&task_context, &old_comments?).await?;
                api::create_comment(&task_context, &issue_number, &Value::String(body))
                    .await
                    .map(|_| ())
            });
        }
        Ok(())
    }
}
