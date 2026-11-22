use crate::error::{Error, Result};
use serde_json::{Map, Value};
use std::borrow::Cow;

#[must_use]
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number
            .as_f64()
            .is_some_and(|float| float != 0.0 && !float.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

#[must_use]
pub fn is_truthy(value: &Value) -> bool {
    truthy(Some(value))
}

#[must_use]
pub fn type_of(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null | Value::Array(_) | Value::Object(_)) => "object",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
    }
}

#[must_use]
pub fn number_to_string(number: &serde_json::Number) -> String {
    if let Some(integer) = number.as_i64() {
        return integer.to_string();
    }
    if let Some(unsigned) = number.as_u64() {
        return unsigned.to_string();
    }
    let float = number.as_f64().unwrap_or(f64::NAN);
    float_to_string(float)
}

#[must_use]
pub fn float_to_string(float: f64) -> String {
    if float.is_nan() {
        return "NaN".to_string();
    }
    if float.is_infinite() {
        return if float > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if float == 0.0 {
        return "0".to_string();
    }
    let scientific = format!("{float:e}");
    let (mantissa, exponent_text) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent_text.parse().unwrap_or(0);
    if (-6..21).contains(&exponent) {
        format!("{float}")
    } else {
        let sign = if exponent >= 0 { "+" } else { "" };
        format!("{mantissa}e{sign}{exponent}")
    }
}

#[must_use]
pub fn to_display_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number_to_string(number),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => {
            let mut joined = String::new();
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    joined.push(',');
                }
                match item {
                    Value::Null => {}
                    Value::String(text) => joined.push_str(text),
                    other => joined.push_str(&to_display_string(Some(other))),
                }
            }
            joined
        }
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

#[must_use]
pub fn stringify(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[must_use]
pub fn stringify_optional(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(present) => stringify(present),
    }
}

#[must_use]
pub fn get<'value>(value: &'value Value, key: &str) -> Option<&'value Value> {
    match value {
        Value::Object(map) => map.get(key),
        Value::Array(items) => key.parse::<usize>().ok().and_then(|index| items.get(index)),
        _ => None,
    }
}

#[must_use]
pub fn get_path<'value>(value: &'value Value, path: &[&str]) -> Option<&'value Value> {
    let mut current = value;
    for key in path {
        current = get(current, key)?;
    }
    Some(current)
}

pub fn read<'value>(value: Option<&'value Value>, key: &str) -> Result<Option<&'value Value>> {
    match value {
        None => Err(Error::missing_property(key)),
        Some(Value::Null) => Err(Error::type_mismatch(format!(
            "Cannot read properties of null (reading '{key}')"
        ))),
        Some(present) => Ok(get(present, key)),
    }
}

#[must_use]
pub fn get_mut<'value>(value: &'value mut Value, key: &str) -> Option<&'value mut Value> {
    match value {
        Value::Object(map) => map.get_mut(key),
        Value::Array(items) => key
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get_mut(index)),
        _ => None,
    }
}

pub fn take(value: Option<&mut Value>, key: &str) -> Result<Option<Value>> {
    read(value.as_deref(), key)?;
    Ok(value
        .and_then(|present| get_mut(present, key))
        .map(Value::take))
}

pub fn take_path(value: &mut Value, path: &[&str]) -> Result<Option<Value>> {
    read_path(value, path)?;
    let mut current = Some(value);
    for key in path {
        current = current.and_then(|present| get_mut(present, key));
    }
    Ok(current.map(Value::take))
}

pub fn read_path<'value>(value: &'value Value, path: &[&str]) -> Result<Option<&'value Value>> {
    let mut current = Some(value);
    for key in path {
        current = read(current, key)?;
    }
    Ok(current)
}

pub fn read_string(value: &Value, path: &[&str]) -> Result<String> {
    Ok(to_display_string(read_path(value, path)?))
}

pub fn as_str(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

pub fn object_keys(value: Option<&Value>) -> Result<Vec<String>> {
    match value {
        None | Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot convert undefined or null to object",
        )),
        Some(Value::Object(map)) => Ok(map.keys().cloned().collect()),
        Some(Value::Array(items)) => Ok((0..items.len()).map(|index| index.to_string()).collect()),
        Some(Value::String(text)) => Ok((0..text.encode_utf16().count())
            .map(|index| index.to_string())
            .collect()),
        Some(_) => Ok(Vec::new()),
    }
}

pub fn includes(container: Option<&Value>, needle: &Value) -> Result<bool> {
    match container {
        Some(Value::Array(items)) => Ok(items.iter().any(|item| strict_equals(item, needle))),
        Some(Value::String(text)) => Ok(text.contains(&to_display_string(Some(needle)))),
        None => Err(Error::missing_property("includes")),
        Some(_) => Err(Error::type_mismatch("includes is not a function")),
    }
}

#[must_use]
pub fn strict_equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(first), Value::Number(second)) => first.as_f64() == second.as_f64(),
        (Value::Array(_), Value::Array(_)) | (Value::Object(_), Value::Object(_)) => false,
        _ => left == right,
    }
}

pub static NULL: Value = Value::Null;

#[must_use]
pub fn as_array_or_wrap(value: &Value) -> &[Value] {
    match value {
        Value::Array(items) => items,
        other => std::slice::from_ref(other),
    }
}

#[must_use]
pub fn into_array_or_wrap(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        other => vec![other],
    }
}

pub fn concat<'value>(
    base: &'value [Value],
    addition: &'value Value,
) -> impl Iterator<Item = &'value Value> {
    base.iter().chain(as_array_or_wrap(addition))
}

#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "lengths and indexes are compared as f64 numbers; they stay far below 2^53 where f64 is exact"
)]
pub fn number_from_length(length: usize) -> f64 {
    length as f64
}

#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "timestamps are compared as f64 milliseconds; valid timestamps stay within 8.64e15 ms where f64 is exact"
)]
pub fn number_from_timestamp_milliseconds(milliseconds: i64) -> f64 {
    milliseconds as f64
}

#[must_use]
pub fn object() -> Map<String, Value> {
    Map::new()
}

fn is_container(value: &Value) -> bool {
    value.is_object() || value.is_array()
}

fn empty_container_like(value: &Value) -> Option<Value> {
    match value {
        Value::Object(_) => Some(Value::Object(Map::new())),
        Value::Array(_) => Some(Value::Array(Vec::new())),
        _ => None,
    }
}

pub fn deep_merge_into(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                match target_map.get_mut(key) {
                    Some(existing) if is_container(existing) && is_container(source_value) => {
                        deep_merge_into(existing, source_value);
                    }
                    Some(existing) if is_container(existing) => {
                        *existing = source_value.clone();
                    }
                    _ => {
                        let merged = match empty_container_like(source_value) {
                            Some(mut fresh) => {
                                deep_merge_into(&mut fresh, source_value);
                                fresh
                            }
                            None => source_value.clone(),
                        };
                        target_map.insert(key.clone(), merged);
                    }
                }
            }
        }
        (Value::Array(target_items), Value::Array(source_items)) => {
            for (index, source_value) in source_items.iter().enumerate() {
                match target_items.get_mut(index) {
                    Some(existing) if is_container(existing) && is_container(source_value) => {
                        deep_merge_into(existing, source_value);
                    }
                    Some(existing) => *existing = source_value.clone(),
                    None => target_items.push(source_value.clone()),
                }
            }
        }
        (target_slot, source_value) => *target_slot = source_value.clone(),
    }
}

fn to_primitive(value: Option<&Value>) -> Option<Cow<'_, Value>> {
    match value {
        Some(Value::Array(_) | Value::Object(_)) => {
            Some(Cow::Owned(Value::String(to_display_string(value))))
        }
        other => other.map(Cow::Borrowed),
    }
}

#[must_use]
pub fn greater_than(value: Option<&Value>, other: Option<&Value>) -> bool {
    less_than(other, value)
}

#[must_use]
pub fn less_than(value: Option<&Value>, other: Option<&Value>) -> bool {
    let left = to_primitive(value);
    let right = to_primitive(other);
    if let (Some(Value::String(left_text)), Some(Value::String(right_text))) =
        (left.as_deref(), right.as_deref())
    {
        return left_text.encode_utf16().lt(right_text.encode_utf16());
    }
    to_number(left.as_deref()) < to_number(right.as_deref())
}

#[must_use]
pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => string_to_number(text),
        Some(Value::Array(_)) => string_to_number(&to_display_string(value)),
    }
}

#[must_use]
pub fn trim_whitespace_and_byte_order_mark(text: &str) -> &str {
    text.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}')
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a radix literal rounds to the nearest f64 exactly like an integer-to-float cast"
)]
fn string_to_number(text: &str) -> f64 {
    let trimmed = trim_whitespace_and_byte_order_mark(text);
    if trimmed.is_empty() {
        return 0.0;
    }
    let radix_prefixes = [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ];
    for (prefix, radix) in radix_prefixes {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).map_or(f64::NAN, |parsed| parsed as f64);
        }
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let is_decimal_literal = trimmed.chars().all(|character| {
        character.is_ascii_digit() || matches!(character, '.' | 'e' | 'E' | '+' | '-')
    });
    if !is_decimal_literal {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_string_joins_arrays_and_formats_numbers() {
        assert_eq!(to_display_string(Some(&json!([1, "a", null]))), "1,a,");
        assert_eq!(to_display_string(Some(&json!({"a": 1}))), "[object Object]");
        assert_eq!(to_display_string(Some(&json!(2.5))), "2.5");
        assert_eq!(to_display_string(Some(&json!(3))), "3");
        assert_eq!(float_to_string(1e21), "1e+21");
        assert_eq!(float_to_string(1.5e-7), "1.5e-7");
        assert_eq!(float_to_string(0.000_001), "0.000001");
        assert_eq!(to_display_string(None), "undefined");
    }

    #[test]
    fn empty_text_zero_and_missing_values_are_falsy() {
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(Some(&json!(0))));
        assert!(truthy(Some(&json!({}))));
        assert!(truthy(Some(&json!([]))));
        assert!(!truthy(None));
    }

    #[test]
    fn reading_a_property_of_a_missing_value_fails_with_a_type_error() {
        let payload = json!({"user": null});
        let error = read_path(&payload, &["pull_request", "user"]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'user')"
        );
    }

    #[test]
    fn deep_merge_merges_arrays_by_index_and_objects_deeply() {
        let mut target = json!({"a": [1, {"b": 1}], "c": {"d": 1}});
        deep_merge_into(&mut target, &json!({"a": [9], "c": {"e": 2}}));
        assert_eq!(target, json!({"a": [9, {"b": 1}], "c": {"d": 1, "e": 2}}));
    }

    #[test]
    fn text_converts_to_number_with_trimming_and_radix_prefixes() {
        assert_eq!(to_number(Some(&json!(" 7 "))), 7.0);
        assert_eq!(to_number(Some(&json!(""))), 0.0);
        assert!(to_number(Some(&json!("inf"))).is_nan());
        assert_eq!(to_number(Some(&json!("0x10"))), 16.0);
        assert!(to_number(None).is_nan());
    }
}
