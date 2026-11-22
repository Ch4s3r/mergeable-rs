use crate::json_value;
use serde_json::Value;
use std::borrow::Borrow;
use std::cmp::Ordering;

#[must_use]
pub fn optional_values_equal(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left_value), Some(right_value)) => json_value::strict_equals(left_value, right_value),
        _ => false,
    }
}

#[must_use]
pub fn contains<Item: Borrow<Value>>(items: &[Item], needle: &Value) -> bool {
    items
        .iter()
        .any(|item| json_value::strict_equals(item.borrow(), needle))
}

fn push_unless_contained<Item: Borrow<Value>>(mut kept: Vec<Item>, item: Item) -> Vec<Item> {
    if !contains(&kept, item.borrow()) {
        kept.push(item);
    }
    kept
}

#[must_use]
pub fn unique<'item>(items: impl IntoIterator<Item = &'item Value>) -> Vec<&'item Value> {
    items.into_iter().fold(Vec::new(), push_unless_contained)
}

#[must_use]
pub fn unique_owned(items: impl IntoIterator<Item = Value>) -> Vec<Value> {
    items.into_iter().fold(Vec::new(), push_unless_contained)
}

#[must_use]
pub fn union<'item>(
    first: impl IntoIterator<Item = &'item Value>,
    second: impl IntoIterator<Item = &'item Value>,
) -> Vec<&'item Value> {
    unique(first.into_iter().chain(second))
}

#[must_use]
pub fn intersection<'item, Other: Borrow<Value>>(
    first: &'item [Value],
    second: &[Other],
) -> Vec<&'item Value> {
    unique(first.iter().filter(|item| contains(second, item)))
}

#[must_use]
pub fn difference<'item, Excluded: Borrow<Value>>(
    items: &'item [Value],
    excluded: &[Excluded],
) -> Vec<&'item Value> {
    items
        .iter()
        .filter(|item| !contains(excluded, item))
        .collect()
}

#[must_use]
pub fn array_items(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

#[must_use]
pub fn unique_by_path(items: Vec<Value>, path: &[&str]) -> Vec<Value> {
    let mut unique: Vec<Value> = Vec::new();
    for item in items {
        let key = json_value::get_path(&item, path);
        let already_seen = unique
            .iter()
            .any(|kept| optional_values_equal(json_value::get_path(kept, path), key));
        if !already_seen {
            unique.push(item);
        }
    }
    unique
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[must_use]
pub fn order_by_path(items: Vec<Value>, path: &[&str], order: SortOrder) -> Vec<Value> {
    let mut indexed: Vec<(usize, Value)> = items.into_iter().enumerate().collect();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        let ascending = compare_ascending(
            json_value::get_path(left, path),
            json_value::get_path(right, path),
        );
        let ordered = match order {
            SortOrder::Ascending => ascending,
            SortOrder::Descending => ascending.reverse(),
        };
        ordered.then(left_index.cmp(right_index))
    });
    indexed.into_iter().map(|(_, item)| item).collect()
}

#[must_use]
pub fn compare_ascending(value: Option<&Value>, other: Option<&Value>) -> Ordering {
    if optional_values_equal(value, other) {
        return Ordering::Equal;
    }
    let value_is_null = matches!(value, Some(Value::Null));
    let other_is_null = matches!(other, Some(Value::Null));
    let value_is_defined = value.is_some();
    let other_is_defined = other.is_some();
    if (!other_is_null && json_value::greater_than(value, other))
        || (value_is_null && other_is_defined)
        || !value_is_defined
    {
        return Ordering::Greater;
    }
    if (!value_is_null && json_value::greater_than(other, value))
        || (other_is_null && value_is_defined)
        || !other_is_defined
    {
        return Ordering::Less;
    }
    Ordering::Equal
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
        let ordered: Vec<Value> = order_by_path(items, &["at"], SortOrder::Descending)
            .iter()
            .map(|item| item["id"].clone())
            .collect();
        assert_eq!(ordered, vec![json!(2), json!(3), json!(1), json!(4)]);
    }

    #[test]
    fn union_and_intersection_keep_first_array_order_without_duplicates() {
        let first = vec![json!("a"), json!("b"), json!("a")];
        let second = vec![json!("c"), json!("b")];
        assert_eq!(
            union(&first, &second),
            vec![&json!("a"), &json!("b"), &json!("c")]
        );
        assert_eq!(intersection(&first, &second), vec![&json!("b")]);
        assert_eq!(difference(&first, &second), vec![&json!("a"), &json!("a")]);
        assert_eq!(
            unique_owned(vec![json!(1), json!(1.0), json!("1"), json!(1)]),
            vec![json!(1), json!("1")]
        );
    }
}
