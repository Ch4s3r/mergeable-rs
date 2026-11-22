use crate::error::{Error, Result};
use crate::json_value;
use crate::results::Status;
use crate::validators::options;
use crate::validators::options::output::OptionResult;
use futures::future::{BoxFuture, join_all};
use serde_json::Value;

const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected array type as input";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connective {
    And,
    Or,
}

impl Connective {
    fn key(self) -> &'static str {
        match self {
            Connective::And => "and",
            Connective::Or => "or",
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Connective::And => " ***AND*** ",
            Connective::Or => " ***OR*** ",
        }
    }

    fn combine(self, previous: bool, next: bool) -> bool {
        match self {
            Connective::And => previous && next,
            Connective::Or => previous || next,
        }
    }
}

async fn validate_filter(
    validator_context: &Value,
    input: &Value,
    filter: &Value,
) -> Result<Option<Value>> {
    for connective in [Connective::And, Connective::Or] {
        if json_value::truthy(json_value::read(Some(filter), connective.key())?) {
            return Ok(Some(
                process(validator_context, input, filter, connective)
                    .await?
                    .into_value(),
            ));
        }
    }
    let mut raw_output = options::process(validator_context, input, filter, true).await?;
    Ok(json_value::get_mut(&mut raw_output, "0").map(Value::take))
}

pub fn process<'call>(
    validator_context: &'call Value,
    input: &'call Value,
    rule: &'call Value,
    connective: Connective,
) -> BoxFuture<'call, Result<OptionResult>> {
    Box::pin(async move {
        let key = connective.key();
        let Some(Value::Array(filters)) = json_value::get(rule, key) else {
            return Err(Error::general(UNKNOWN_INPUT_TYPE_ERROR));
        };
        let validated: Vec<Option<Value>> = join_all(
            filters
                .iter()
                .map(|filter| validate_filter(validator_context, input, filter)),
        )
        .await
        .into_iter()
        .collect::<Result<_>>()?;

        let mut is_mergeable: Option<bool> = None;
        let mut descriptions = String::new();
        let mut does_error_exist = false;
        let mut error_message = String::from("Error occurred: \n");

        for result in &validated {
            let status = json_value::read(result.as_ref(), "status")?
                .and_then(Value::as_str)
                .and_then(Status::parse);
            let description_text = json_value::to_display_string(
                result
                    .as_ref()
                    .and_then(|present| json_value::get(present, "description")),
            );
            if status == Some(Status::Error) {
                does_error_exist = true;
                error_message.push_str("- ");
                error_message.push_str(&description_text);
                error_message.push_str(" \n");
            }
            let result_success = status == Some(Status::Pass);
            is_mergeable = Some(match is_mergeable {
                Some(previous) => connective.combine(previous, result_success),
                None => result_success,
            });
            if status == Some(Status::Fail) {
                if descriptions.encode_utf16().count() > 2 {
                    descriptions.push(' ');
                    descriptions.push_str(connective.separator());
                    descriptions.push(' ');
                    descriptions.push_str(&description_text);
                } else {
                    descriptions.push_str(&description_text);
                }
            }
        }

        if does_error_exist {
            return Ok(OptionResult {
                status: Status::Error,
                description: Value::String(error_message),
            });
        }
        let success_description =
            format!("All the requisite validations passed for '{key}' option");
        Ok(OptionResult::decided(
            is_mergeable.unwrap_or(false),
            success_description,
            format!("({descriptions})"),
        ))
    })
}
