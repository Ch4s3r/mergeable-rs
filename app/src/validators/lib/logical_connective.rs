use crate::context::Context;
use crate::error::{Error, Result};
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use crate::validators::options::lib::construct_error_output;
use crate::workflow;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalOperator {
    And,
    Or,
    Not,
}

impl LogicalOperator {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LogicalOperator::And => "And",
            LogicalOperator::Or => "Or",
            LogicalOperator::Not => "Not",
        }
    }
}

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
    operator: LogicalOperator,
) -> Result<Value> {
    let validator_name = operator.name();
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

    let output = workflow::validator_results(context, registry, &rules).await?;

    let mut validations_out = Vec::new();
    let mut status = if operator == LogicalOperator::And {
        "pass"
    } else {
        "fail"
    };
    let output_count = output.len();
    let negated_first_status = match output
        .first()
        .and_then(|first| json_value::as_str(first.get("status")))
    {
        Some("error") => Some("error"),
        Some("pass") => Some("fail"),
        Some("fail") => Some("pass"),
        _ => None,
    };
    for (count, mut result) in output.into_iter().enumerate() {
        let result_status = json_value::as_str(result.get("status"));
        if result_status == Some("error") {
            status = "error";
        }
        if operator == LogicalOperator::Or && result_status == Some("pass") && status != "error" {
            status = "pass";
        }
        if operator == LogicalOperator::And && result_status == Some("fail") && status != "error" {
            status = "fail";
        }
        let result_name = json_value::to_display_string(result.get("name"));
        let Some(Value::Array(result_validations)) = result.get_mut("validations").map(Value::take)
        else {
            return Err(Error::type_mismatch("result.validations is not iterable"));
        };
        for mut renamed in result_validations {
            let description = json_value::to_display_string(renamed.get("description"));
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
    if operator == LogicalOperator::Not {
        if output_count != 1 {
            return Ok(error_result(
                validator_name,
                settings,
                &format!(
                    "Failed to validate because the 'validate' option does not have exactly one element, but {output_count}"
                ),
            ));
        }
        final_status = negated_first_status;
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
