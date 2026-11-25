pub mod assignees;
pub mod deep_validation;
pub mod git_pattern;
pub mod list_processor;
pub mod owners;
pub mod requested_reviewers;
pub mod teams;

use crate::error::{Error, Result};
use crate::json_value;
use serde_json::Value;

pub fn map_logins(list: Option<&Value>, list_expression: &str) -> Result<Vec<Value>> {
    match list {
        Some(Value::Array(users)) => users
            .iter()
            .map(|user| {
                Ok(json_value::read(Some(user), "login")?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect(),
        None => Err(Error::missing_property("map")),
        Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'map')",
        )),
        Some(_) => Err(Error::type_mismatch(format!(
            "{list_expression}.map is not a function"
        ))),
    }
}

#[must_use]
pub fn unique_values(values: Vec<Value>) -> Vec<Value> {
    let mut unique: Vec<Value> = Vec::new();
    for value in values {
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    unique
}
