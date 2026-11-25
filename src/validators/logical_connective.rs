use crate::context::Context;
use crate::error::{Error, Result};
use crate::json_value;
use crate::registry::Registry;
use crate::results::{self, Connective, Status};
use crate::validators::options::output::construct_error_output;
use crate::workflow;
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

fn prefix_descriptions(mut result: Value, option_number: usize) -> Result<Vec<Value>> {
    let result_name = json_value::to_display_string(result.get("name"));
    let Some(Value::Array(sub_validations)) = result.get_mut("validations").map(Value::take) else {
        return Err(Error::type_mismatch("result.validations is not iterable"));
    };
    Ok(sub_validations
        .into_iter()
        .map(|mut sub_validation| {
            let description = json_value::to_display_string(sub_validation.get("description"));
            if let Value::Object(fields) = &mut sub_validation {
                fields.insert(
                    "description".to_string(),
                    Value::String(format!(
                        "Option {option_number}: {result_name}: {description}"
                    )),
                );
            }
            sub_validation
        })
        .collect())
}

pub async fn process(
    context: &Context,
    settings: Option<&Value>,
    registry: &Registry,
    connective: Connective,
) -> Result<Value> {
    let validator_name = connective.name();
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
    let statuses: Vec<Option<Status>> = output.iter().map(Status::of).collect();
    let sub_validation_count = output.len();

    let mut prefixed_validations = Vec::new();
    for (option_index, result) in output.into_iter().enumerate() {
        prefixed_validations.extend(prefix_descriptions(result, option_index + 1)?);
    }

    if connective == Connective::Not && sub_validation_count != 1 {
        return Ok(error_result(
            validator_name,
            settings,
            &format!(
                "Failed to validate because the 'validate' option does not have exactly one element, but {sub_validation_count}"
            ),
        ));
    }

    let mut combined = Map::new();
    if let Some(status) = connective.combined_status(&statuses) {
        combined.insert("status".to_string(), status.into());
    }
    combined.insert("name".to_string(), json!(validator_name));
    combined.insert(
        "validations".to_string(),
        Value::Array(prefixed_validations),
    );
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
