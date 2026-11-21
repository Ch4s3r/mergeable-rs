use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct JsError {
    pub name: String,
    pub message: String,
    pub status: Option<u16>,
    pub response_data: Option<Box<Value>>,
}

impl JsError {
    pub fn named(name: impl Into<String>, message: impl Into<String>) -> Self {
        JsError {
            name: name.into(),
            message: message.into(),
            status: None,
            response_data: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::named("Error", message)
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::named("TypeError", message)
    }

    pub fn reading_property_of_undefined(property: &str) -> Self {
        Self::type_error(format!(
            "Cannot read properties of undefined (reading '{property}')"
        ))
    }

    pub fn unsupported_setting(message: impl Into<String>) -> Self {
        Self::named("UnSupportedSettingError", message)
    }

    pub fn team_not_found(message: impl Into<String>) -> Self {
        Self::named("TeamNotFoundError", message)
    }

    pub fn http(status: u16, message: impl Into<String>, response_data: Option<Value>) -> Self {
        JsError {
            name: "HttpError".to_string(),
            message: message.into(),
            status: Some(status),
            response_data: response_data.map(Box::new),
        }
    }

    pub fn is_named(&self, name: &str) -> bool {
        self.name == name
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(formatter, "{}", self.name)
        } else {
            write!(formatter, "{}: {}", self.name, self.message)
        }
    }
}

impl std::error::Error for JsError {}

pub type JsResult<T> = Result<T, JsError>;
