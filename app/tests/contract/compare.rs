use super::normalize::canonical_text;
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub enum CallComparison {
    Equal,
    OrderDiffers {
        first_differing_position: usize,
    },
    Different {
        first_differing_position: usize,
        expected: Option<Value>,
        actual: Option<Value>,
    },
}

fn first_differing_position(expected: &[Value], actual: &[Value]) -> usize {
    expected
        .iter()
        .zip(actual.iter())
        .position(|(expected_call, actual_call)| {
            canonical_text(expected_call) != canonical_text(actual_call)
        })
        .unwrap_or(expected.len().min(actual.len()))
}

fn sorted_canonical_texts(calls: &[Value]) -> Vec<String> {
    let mut texts: Vec<String> = calls.iter().map(canonical_text).collect();
    texts.sort();
    texts
}

#[must_use]
pub fn compare_calls(expected: &[Value], actual: &[Value]) -> CallComparison {
    let position = first_differing_position(expected, actual);
    if expected.len() == actual.len() && position == expected.len() {
        return CallComparison::Equal;
    }
    if sorted_canonical_texts(expected) == sorted_canonical_texts(actual) {
        return CallComparison::OrderDiffers {
            first_differing_position: position,
        };
    }
    CallComparison::Different {
        first_differing_position: position,
        expected: expected.get(position).cloned(),
        actual: actual.get(position).cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn calls_with_reordered_body_keys_are_equal() {
        let expected = vec![json!({"method": "POST", "body": {"a": 1, "b": 2}})];
        let actual = vec![json!({"body": {"b": 2, "a": 1}, "method": "POST"})];
        assert_eq!(compare_calls(&expected, &actual), CallComparison::Equal);
    }

    #[test]
    fn swapped_calls_only_differ_in_order() {
        let first = json!({"method": "GET", "path": "/a"});
        let second = json!({"method": "GET", "path": "/b"});
        assert_eq!(
            compare_calls(&[first.clone(), second.clone()], &[second, first]),
            CallComparison::OrderDiffers {
                first_differing_position: 0
            }
        );
    }

    #[test]
    fn missing_call_is_reported_at_its_position() {
        let first = json!({"method": "GET", "path": "/a"});
        let second = json!({"method": "PATCH", "path": "/b"});
        assert_eq!(
            compare_calls(&[first.clone(), second.clone()], &[first]),
            CallComparison::Different {
                first_differing_position: 1,
                expected: Some(second),
                actual: None
            }
        );
    }
}
