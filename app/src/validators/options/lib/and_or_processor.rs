use crate::error::{JsError, JsResult};
use crate::js;
use crate::validators::options;
use crate::validators::options::lib::OptionResult;
use futures::future::{BoxFuture, join_all};
use serde_json::Value;

const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected array type as input";

async fn validate_filter(
    validator_context: &Value,
    input: &Value,
    filter: &Value,
) -> JsResult<Option<Value>> {
    if js::truthy(js::read(Some(filter), "and")?) {
        return Ok(Some(
            process(validator_context, input, filter, "and")
                .await?
                .to_value(),
        ));
    }
    if js::truthy(js::read(Some(filter), "or")?) {
        return Ok(Some(
            process(validator_context, input, filter, "or")
                .await?
                .to_value(),
        ));
    }
    let raw_output = options::process(validator_context, input, filter, true).await?;
    Ok(js::get(&raw_output, "0").cloned())
}

pub fn process<'call>(
    validator_context: &'call Value,
    input: &'call Value,
    rule: &'call Value,
    key: &'call str,
) -> BoxFuture<'call, JsResult<OptionResult>> {
    Box::pin(async move {
        let Some(Value::Array(filters)) = js::get(rule, key) else {
            return Err(JsError::error(UNKNOWN_INPUT_TYPE_ERROR));
        };
        let validated: Vec<Option<Value>> = join_all(
            filters
                .iter()
                .map(|filter| validate_filter(validator_context, input, filter)),
        )
        .await
        .into_iter()
        .collect::<JsResult<_>>()?;

        let is_and = key == "and";
        let mut is_mergeable: Option<bool> = None;
        let mut descriptions = String::new();
        let mut does_error_exist = false;
        let mut error_message = String::from("Error occurred: \n");

        for result in &validated {
            let status = js::read(result.as_ref(), "status")?;
            let description_text = js::to_js_string(
                result
                    .as_ref()
                    .and_then(|present| js::get(present, "description")),
            );
            let status_text = js::as_str(status);
            if status_text == Some("error") {
                does_error_exist = true;
                error_message.push_str("- ");
                error_message.push_str(&description_text);
                error_message.push_str(" \n");
            }
            let result_success = status_text == Some("pass");
            is_mergeable = Some(match is_mergeable {
                Some(previous) if is_and => previous && result_success,
                Some(previous) => previous || result_success,
                None => result_success,
            });
            if status_text == Some("fail") {
                if descriptions.encode_utf16().count() > 2 {
                    let connective = if is_and { " ***AND*** " } else { " ***OR*** " };
                    descriptions.push(' ');
                    descriptions.push_str(connective);
                    descriptions.push(' ');
                    descriptions.push_str(&description_text);
                } else {
                    descriptions.push_str(&description_text);
                }
            }
        }

        if does_error_exist {
            return Ok(OptionResult {
                status: "error",
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
