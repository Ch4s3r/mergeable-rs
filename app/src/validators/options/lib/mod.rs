pub mod and_or_processor;
pub mod js_regex;

use crate::error::{JsError, JsResult};
use crate::js;
use js_regex::{JsRegex, JsRegexFlags};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct OptionResult {
    pub status: &'static str,
    pub description: Value,
}

impl OptionResult {
    pub fn decided(
        is_mergeable: bool,
        success_description: impl Into<Value>,
        failure_description: impl Into<Value>,
    ) -> Self {
        if is_mergeable {
            OptionResult {
                status: "pass",
                description: success_description.into(),
            }
        } else {
            OptionResult {
                status: "fail",
                description: failure_description.into(),
            }
        }
    }

    pub fn to_value(&self) -> Value {
        json!({"status": self.status, "description": self.description})
    }
}

pub fn construct_output(
    validator_context: &Value,
    input: Option<&Value>,
    settings: Option<&Value>,
    status: &str,
    description: Value,
    error: Option<Value>,
) -> Value {
    let mut details = Map::new();
    if let Some(present_input) = input {
        details.insert("input".to_string(), present_input.clone());
    }
    if let Some(present_settings) = settings {
        details.insert("settings".to_string(), present_settings.clone());
    }
    if let Some(present_error) = error {
        details.insert("error".to_string(), present_error);
    }
    json!({
        "validator": validator_context,
        "status": status,
        "description": description,
        "details": Value::Object(details),
    })
}

pub fn construct_error_output(
    validator_context: &Value,
    input: Option<&Value>,
    settings: Option<&Value>,
    description: &str,
) -> Value {
    construct_output(
        validator_context,
        input,
        settings,
        "error",
        Value::String(description.to_string()),
        None,
    )
}

pub fn validator_name(validator_context: &Value) -> String {
    js::to_js_string(js::get(validator_context, "name"))
}

pub fn message_or_default(message: Option<&Value>, default_description: String) -> Value {
    match message {
        Some(custom) if js::is_truthy(custom) => custom.clone(),
        _ => Value::String(default_description),
    }
}

pub fn regex_flags(filter: &Value) -> String {
    match js::get(filter, "regex_flag") {
        Some(flag) if js::is_truthy(flag) => {
            if flag.as_str() == Some("none") {
                String::new()
            } else {
                js::to_js_string(Some(flag))
            }
        }
        _ => "i".to_string(),
    }
}

pub fn compile_user_regex(regex: &Value, flags: &str) -> JsResult<JsRegex> {
    let pattern = js::to_js_string(Some(regex));
    JsRegex::new(&pattern, flags).map_err(|_| {
        JsError::error(format!(
            "Failed to create a regex expression with the provided regex: {pattern}"
        ))
    })
}

pub fn compile_constant_pattern(pattern: &str, flags: JsRegexFlags) -> Option<fancy_regex::Regex> {
    let translated = js_regex::translate(pattern, flags).ok()?;
    fancy_regex::Regex::new(&translated).ok()
}

pub fn concat_into_list(value: &Value) -> Vec<Value> {
    js::as_array_or_wrap(value)
}

pub fn join_list(items: &[Value], separator: &str) -> String {
    items
        .iter()
        .map(|item| match item {
            Value::Null => String::new(),
            other => js::to_js_string(Some(other)),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

pub fn utf16_length(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

pub fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Affix {
    Prefix,
    Suffix,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotSearchable;

fn index_of(container: &Value, needle: &Value) -> Result<f64, NotSearchable> {
    match container {
        Value::String(text) => {
            let needle_text = js::to_js_string(Some(needle));
            Ok(text.find(&needle_text).map_or(-1.0, |byte_offset| {
                utf16_length(&text[..byte_offset]) as f64
            }))
        }
        Value::Array(items) => Ok(items
            .iter()
            .position(|item| js::strict_equals(item, needle))
            .map_or(-1.0, |index| index as f64)),
        _ => Err(NotSearchable),
    }
}

fn length_of(value: &Value) -> Option<f64> {
    match value {
        Value::String(text) => Some(utf16_length(text) as f64),
        Value::Array(items) => Some(items.len() as f64),
        _ => None,
    }
}

fn has_affix(container: &Value, item: &Value, affix: Affix) -> Result<bool, NotSearchable> {
    let index = index_of(container, item)?;
    Ok(match affix {
        Affix::Prefix => index == 0.0,
        Affix::Suffix => match (length_of(container), length_of(item)) {
            (Some(container_length), Some(item_length)) => index == container_length - item_length,
            _ => false,
        },
    })
}

fn any_match_item(
    match_value: &Value,
    mut check: impl FnMut(&Value) -> Result<bool, NotSearchable>,
) -> Result<bool, NotSearchable> {
    match match_value {
        Value::Array(items) => {
            for item in items {
                if check(item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        single => check(single),
    }
}

pub fn input_has_affix(
    input: &Value,
    match_value: &Value,
    affix: Affix,
) -> Result<bool, NotSearchable> {
    if !matches!(input, Value::String(_) | Value::Array(_))
        || !matches!(match_value, Value::String(_) | Value::Array(_))
    {
        return Err(NotSearchable);
    }
    match input {
        Value::Array(input_items) => {
            for input_item in input_items {
                if any_match_item(match_value, |match_item| {
                    has_affix(input_item, match_item, affix)
                })? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        text => any_match_item(match_value, |match_item| has_affix(text, match_item, affix)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ends_with_uses_the_first_occurrence_like_javascript_index_of() {
        assert_eq!(
            input_has_affix(&json!("test test"), &json!("test"), Affix::Suffix),
            Ok(false)
        );
        assert_eq!(
            input_has_affix(&json!("the test"), &json!("test"), Affix::Suffix),
            Ok(true)
        );
    }

    #[test]
    fn affix_check_rejects_non_string_input() {
        assert_eq!(
            input_has_affix(&json!(5), &json!("a"), Affix::Prefix),
            Err(NotSearchable)
        );
        assert_eq!(
            input_has_affix(&json!([5]), &json!("a"), Affix::Prefix),
            Err(NotSearchable)
        );
    }

    #[test]
    fn list_join_turns_null_into_empty_text() {
        assert_eq!(
            join_list(&[json!("a"), Value::Null, json!(1)], ", "),
            "a, , 1"
        );
    }

    #[test]
    fn javascript_trim_removes_byte_order_mark_but_not_next_line() {
        assert_eq!(js_trim("\u{FEFF} a \u{A0}"), "a");
        assert_eq!(js_trim("\u{85}"), "\u{85}");
    }
}
