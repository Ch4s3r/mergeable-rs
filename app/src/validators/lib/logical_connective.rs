use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::flex;
use crate::js;
use crate::registry::Registry;
use crate::results;
use crate::validators::options::lib::construct_error_output;
use serde_json::{Map, Value, json};

const OPTION_MISSING_ERROR_MESSAGE: &str = "Failed to validate because the 'validate' option is missing or empty. Please check the documentation.";

fn error_result(validator_name: &str, settings: Option<&Value>, description: &str) -> Value {
    let output = construct_error_output(
        &json!(validator_name),
        Some(&json!("")),
        settings,
        description,
    );
    results::consolidate_result(vec![output], &json!({"name": validator_name}))
}

pub async fn process(
    context: &Context,
    settings: Option<&Value>,
    registry: &Registry,
    validator_name: &str,
) -> JsResult<Value> {
    let Some(Value::Array(validations)) =
        settings.filter(|present| matches!(present, Value::Array(items) if !items.is_empty()))
    else {
        return Ok(error_result(
            validator_name,
            settings,
            OPTION_MISSING_ERROR_MESSAGE,
        ));
    };
    let rules = json!({"validate": validations});

    if let Err(error) = registry.register_validators(&rules) {
        return Ok(error_result(
            validator_name,
            settings,
            &format!("Unsupported validator {error}"),
        ));
    }

    let output = flex::validator_results(context, registry, &rules).await?;

    let mut validations_out = Vec::new();
    let mut status = if validator_name == "And" {
        "pass"
    } else {
        "fail"
    };
    for (count, result) in output.iter().enumerate() {
        let result_status = js::as_str(result.get("status"));
        if result_status == Some("error") {
            status = "error";
        }
        if validator_name == "Or" && result_status == Some("pass") && status != "error" {
            status = "pass";
        }
        if validator_name == "And" && result_status == Some("fail") && status != "error" {
            status = "fail";
        }
        let result_name = js::to_js_string(result.get("name"));
        let Some(Value::Array(result_validations)) = result.get("validations") else {
            return Err(JsError::type_error("result.validations is not iterable"));
        };
        for validation in result_validations {
            let mut renamed = validation.clone();
            let description = js::to_js_string(validation.get("description"));
            if let Value::Object(fields) = &mut renamed {
                fields.insert(
                    "description".to_string(),
                    Value::String(format!(
                        "Option {}: {result_name}: {description}",
                        count + 1
                    )),
                );
            }
            validations_out.push(renamed);
        }
    }

    let mut final_status = Some(status);
    if validator_name == "Not" {
        if output.len() != 1 {
            return Ok(error_result(
                validator_name,
                settings,
                &format!(
                    "Failed to validate because the 'validate' option does not have exactly one element, but {}",
                    output.len()
                ),
            ));
        }
        final_status = match js::as_str(output[0].get("status")) {
            Some("error") => Some("error"),
            Some("pass") => Some("fail"),
            Some("fail") => Some("pass"),
            _ => None,
        };
    }

    let mut combined = Map::new();
    if let Some(present_status) = final_status {
        combined.insert("status".to_string(), json!(present_status));
    }
    combined.insert("name".to_string(), json!(validator_name));
    combined.insert("validations".to_string(), Value::Array(validations_out));
    Ok(Value::Object(combined))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_validate_option_is_reported_as_error_without_settings() {
        let result = error_result("And", None, OPTION_MISSING_ERROR_MESSAGE);
        assert_eq!(
            result,
            json!({"status": "error", "name": "And", "validations": [{"validator": "And", "status": "error", "description": OPTION_MISSING_ERROR_MESSAGE, "details": {"input": ""}}]})
        );
    }
}
