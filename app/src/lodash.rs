use crate::js;
use serde_json::Value;
use std::borrow::Cow;
use std::cmp::Ordering;

pub fn same_value_zero(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left_value), Some(right_value)) => js::strict_equals(left_value, right_value),
        _ => false,
    }
}

fn contains(items: &[Value], needle: &Value) -> bool {
    items.iter().any(|item| js::strict_equals(item, needle))
}

fn unique_clones<'item>(items: impl IntoIterator<Item = &'item Value>) -> Vec<Value> {
    let mut unique: Vec<Value> = Vec::new();
    for item in items {
        if !contains(&unique, item) {
            unique.push(item.clone());
        }
    }
    unique
}

pub fn uniq(items: &[Value]) -> Vec<Value> {
    unique_clones(items)
}

pub fn union(first: &[Value], second: &[Value]) -> Vec<Value> {
    unique_clones(first.iter().chain(second))
}

pub fn intersection(first: &[Value], second: &[Value]) -> Vec<Value> {
    unique_clones(first.iter().filter(|item| contains(second, item)))
}

pub fn difference(items: &[Value], excluded: &[Value]) -> Vec<Value> {
    items
        .iter()
        .filter(|item| !contains(excluded, item))
        .cloned()
        .collect()
}

pub fn array_like(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

pub fn uniq_by_path(items: Vec<Value>, path: &[&str]) -> Vec<Value> {
    let mut unique: Vec<Value> = Vec::new();
    for item in items {
        let key = js::get_path(&item, path);
        let already_seen = unique
            .iter()
            .any(|kept| same_value_zero(js::get_path(kept, path), key));
        if !already_seen {
            unique.push(item);
        }
    }
    unique
}

pub fn order_by_path(items: Vec<Value>, path: &[&str], descending: bool) -> Vec<Value> {
    let mut indexed: Vec<(usize, Value)> = items.into_iter().enumerate().collect();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        let ascending = compare_ascending(js::get_path(left, path), js::get_path(right, path));
        let ordered = if descending {
            ascending.reverse()
        } else {
            ascending
        };
        ordered.then(left_index.cmp(right_index))
    });
    indexed.into_iter().map(|(_, item)| item).collect()
}

pub fn compare_ascending(value: Option<&Value>, other: Option<&Value>) -> Ordering {
    if same_value_zero(value, other) {
        return Ordering::Equal;
    }
    let value_is_null = matches!(value, Some(Value::Null));
    let other_is_null = matches!(other, Some(Value::Null));
    let value_is_defined = value.is_some();
    let other_is_defined = other.is_some();
    if (!other_is_null && greater_than(value, other))
        || (value_is_null && other_is_defined)
        || !value_is_defined
    {
        return Ordering::Greater;
    }
    if (!value_is_null && greater_than(other, value))
        || (other_is_null && value_is_defined)
        || !other_is_defined
    {
        return Ordering::Less;
    }
    Ordering::Equal
}

fn to_primitive(value: Option<&Value>) -> Option<Cow<'_, Value>> {
    match value {
        Some(Value::Array(_) | Value::Object(_)) => {
            Some(Cow::Owned(Value::String(js::to_js_string(value))))
        }
        other => other.map(Cow::Borrowed),
    }
}

pub fn greater_than(value: Option<&Value>, other: Option<&Value>) -> bool {
    less_than(other, value)
}

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

pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => string_to_number(text),
        Some(Value::Array(_)) => string_to_number(&js::to_js_string(value)),
    }
}

pub fn js_trim(text: &str) -> &str {
    text.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}')
}

fn string_to_number(text: &str) -> f64 {
    let trimmed = js_trim(text);
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
    fn order_by_descending_puts_missing_values_first_and_keeps_ties_stable() {
        let items = vec![
            json!({"id": 1, "at": "2024-01-01"}),
            json!({"id": 2}),
            json!({"id": 3, "at": "2024-01-03"}),
            json!({"id": 4, "at": "2024-01-01"}),
        ];
        let ordered: Vec<Value> = order_by_path(items, &["at"], true)
            .iter()
            .map(|item| item["id"].clone())
            .collect();
        assert_eq!(ordered, vec![json!(2), json!(3), json!(1), json!(4)]);
    }

    #[test]
    fn string_to_number_follows_javascript_number_conversion() {
        assert_eq!(to_number(Some(&json!(" 7 "))), 7.0);
        assert_eq!(to_number(Some(&json!(""))), 0.0);
        assert!(to_number(Some(&json!("inf"))).is_nan());
        assert_eq!(to_number(Some(&json!("0x10"))), 16.0);
        assert!(to_number(None).is_nan());
    }

    #[test]
    fn union_and_intersection_keep_first_array_order_without_duplicates() {
        let first = vec![json!("a"), json!("b"), json!("a")];
        let second = vec![json!("c"), json!("b")];
        assert_eq!(
            union(&first, &second),
            vec![json!("a"), json!("b"), json!("c")]
        );
        assert_eq!(intersection(&first, &second), vec![json!("b")]);
        assert_eq!(difference(&first, &second), vec![json!("a"), json!("a")]);
    }
}
