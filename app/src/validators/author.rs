use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::js;
use crate::registry::Registry;
use crate::validators::options_processor::{list_processor, teams};
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct Author;

#[async_trait]
impl Validator for Author {
    fn name(&self) -> &'static str {
        "author"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[
            "pull_request.*",
            "pull_request_review.*",
            "issues.*",
            "issue_comment.*",
        ]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": "string", "regex_flag": "string", "message": "string"},
            "team": "string",
            "one_of": "array",
            "none_of": "array"
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let payload = event_aware::get_payload(context);
        let mut author_settings = settings.clone();

        if js::truthy(js::read(Some(&author_settings), "team")?) {
            let result = teams::process_team_option(context, &author_settings, payload).await?;
            if js::as_str(result.get("status")) != Some("pass") {
                return Ok(result);
            }
            if let Value::Object(fields) = &mut author_settings {
                fields.shift_remove("team");
            }
        }

        for list_option in ["one_of", "none_of"] {
            if let Some(list) =
                js::get(&author_settings, list_option).filter(|list| js::is_truthy(list))
            {
                let candidates = list_processor::process(list, context).await?;
                if let Value::Object(fields) = &mut author_settings {
                    fields.insert(list_option.to_string(), Value::Array(candidates));
                }
            }
        }

        let login = js::read(js::read(payload, "user")?, "login")?.unwrap_or(&Value::Null);
        validators::process_options(&author_settings, login, None).await
    }
}
