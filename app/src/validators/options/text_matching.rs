use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::user_pattern::{self, PatternFlags, UserPattern};
use serde_json::Value;

#[must_use]
pub fn regex_flags(filter: &Value) -> String {
    match json_value::get(filter, "regex_flag") {
        Some(flag) if json_value::is_truthy(flag) => {
            if flag.as_str() == Some("none") {
                String::new()
            } else {
                json_value::to_display_string(Some(flag))
            }
        }
        _ => "i".to_string(),
    }
}

pub fn compile_user_regex(regex: &Value, flags: &str) -> Result<UserPattern> {
    let pattern = json_value::to_display_string(Some(regex));
    UserPattern::new(&pattern, flags).map_err(|_| {
        Error::general(format!(
            "Failed to create a regex expression with the provided regex: {pattern}"
        ))
    })
}

#[must_use]
pub fn compile_constant_pattern(pattern: &str, flags: PatternFlags) -> Option<fancy_regex::Regex> {
    let translated = user_pattern::translate(pattern, flags).ok()?;
    fancy_regex::Regex::new(&translated).ok()
}

#[must_use]
pub fn concat_into_list(value: &Value) -> &[Value] {
    json_value::as_array_or_wrap(value)
}

#[must_use]
pub fn join_list(items: &[Value], separator: &str) -> String {
    items
        .iter()
        .map(|item| match item {
            Value::Null => String::new(),
            other => json_value::to_display_string(Some(other)),
        })
        .collect::<Vec<_>>()
        .join(separator)
}

#[must_use]
pub fn utf16_length(text: &str) -> usize {
    text.encode_utf16().count()
}

#[must_use]
pub fn is_trimmable_whitespace(character: char) -> bool {
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

pub fn trim_whitespace(text: &str) -> &str {
    text.trim_matches(is_trimmable_whitespace)
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
            let needle_text = json_value::to_display_string(Some(needle));
            Ok(text.find(&needle_text).map_or(-1.0, |byte_offset| {
                json_value::number_from_length(utf16_length(&text[..byte_offset]))
            }))
        }
        Value::Array(items) => Ok(items
            .iter()
            .position(|item| json_value::strict_equals(item, needle))
            .map_or(-1.0, json_value::number_from_length)),
        _ => Err(NotSearchable),
    }
}

fn length_of(value: &Value) -> Option<f64> {
    match value {
        Value::String(text) => Some(json_value::number_from_length(utf16_length(text))),
        Value::Array(items) => Some(json_value::number_from_length(items.len())),
        _ => None,
    }
}

#[expect(
    clippy::float_cmp,
    reason = "exact comparison of index positions, which are integral numbers"
)]
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
    use serde_json::json;

    #[test]
    fn ends_with_uses_the_first_occurrence_of_the_suffix() {
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
    fn trim_removes_byte_order_mark_but_not_next_line() {
        assert_eq!(trim_whitespace("\u{FEFF} a \u{A0}"), "a");
        assert_eq!(trim_whitespace("\u{85}"), "\u{85}");
    }
}
