use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::validators::options_processor::{list_processor, teams};
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::borrow::Cow;

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

    fn supported_settings(&self) -> Value {
        json!({
            "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": "string", "regex_flag": "string", "message": "string"},
            "team": "string",
            "one_of": "array",
            "none_of": "array"
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let payload = event_aware::get_payload(context);
        let mut author_settings = Cow::Borrowed(settings);

        if json_value::truthy(json_value::read(Some(&*author_settings), "team")?) {
            let result = teams::process_team_option(context, &author_settings, payload).await?;
            if json_value::as_str(result.get("status")) != Some("pass") {
                return Ok(result);
            }
            if let Value::Object(fields) = author_settings.to_mut() {
                fields.shift_remove("team");
            }
        }

        for list_option in ["one_of", "none_of"] {
            if let Some(list) = json_value::get(&author_settings, list_option)
                .filter(|list| json_value::is_truthy(list))
            {
                let candidates = list_processor::process(list, context).await?;
                if let Value::Object(fields) = author_settings.to_mut() {
                    fields.insert(list_option.to_string(), Value::Array(candidates));
                }
            }
        }

        let login =
            json_value::read(json_value::read(payload, "user")?, "login")?.unwrap_or(&Value::Null);
        validators::process_options(&author_settings, login, None).await
    }
}
