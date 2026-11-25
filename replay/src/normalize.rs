use anyhow::Result;
use regex::Regex;
use serde_json::{Map, Value};

pub const TIMESTAMP_PLACEHOLDER: &str = "<timestamp>";
pub const MOCK_ORIGIN_PLACEHOLDER: &str = "http://github.mock";

pub struct RecordedValueNormalizer {
    iso_timestamp: Regex,
    mock_origin: String,
}

impl RecordedValueNormalizer {
    pub fn new(mock_origin: &str) -> Result<RecordedValueNormalizer> {
        Ok(RecordedValueNormalizer {
            mock_origin: mock_origin.to_owned(),
            iso_timestamp: Regex::new(
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?",
            )?,
        })
    }

    #[must_use]
    pub fn normalize_text(&self, text: &str) -> String {
        let without_origin = text.replace(&self.mock_origin, MOCK_ORIGIN_PLACEHOLDER);
        self.iso_timestamp
            .replace_all(&without_origin, TIMESTAMP_PLACEHOLDER)
            .into_owned()
    }

    #[must_use]
    pub fn normalize(&self, value: Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.normalize_text(&text)),
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|item| self.normalize(item)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(key, item)| (key, self.normalize(item)))
                    .collect(),
            ),
            other => other,
        }
    }
}

static REQUIRE_STACK: std::sync::LazyLock<Option<Regex>> =
    std::sync::LazyLock::new(|| Regex::new(r"\nRequire stack:(?:\n- [^\n]*?\.js)*").ok());
static STACK_FRAMES: std::sync::LazyLock<Option<Regex>> =
    std::sync::LazyLock::new(|| Regex::new(r"(?:\n {4}at [^\n]*)+").ok());

#[must_use]
pub fn without_stack_traces(text: &str) -> String {
    let mut stripped = text.to_owned();
    for pattern in [&*REQUIRE_STACK, &*STACK_FRAMES].into_iter().flatten() {
        stripped = pattern.replace_all(&stripped, "").into_owned();
    }
    stripped
}

pub fn with_sorted_keys(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(without_stack_traces(text)),
        Value::Array(items) => Value::Array(items.iter().map(with_sorted_keys).collect()),
        Value::Object(fields) => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                if let Some(item) = fields.get(key) {
                    sorted.insert(key.clone(), with_sorted_keys(item));
                }
            }
            Value::Object(sorted)
        }
        other => other.clone(),
    }
}

#[must_use]
pub fn canonical_text(value: &Value) -> String {
    with_sorted_keys(value).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timestamps_inside_bodies_become_placeholders() -> Result<()> {
        let normalizer = RecordedValueNormalizer::new("http://127.0.0.1:1234")?;
        let body = json!({"started_at": "2026-11-21T20:45:13.893Z", "summary": "at 2024-01-02T03:04:05+01:00 done", "count": 3});
        assert_eq!(
            normalizer.normalize(body),
            json!({"started_at": "<timestamp>", "summary": "at <timestamp> done", "count": 3})
        );
        Ok(())
    }

    #[test]
    fn stack_traces_are_ignored_when_comparing() {
        let recorded = json!({"text": "Error: Cannot find module './validators/x'\nRequire stack:\n- /a/lib/register.js\n- /a/index.js***\nmore"});
        let replayed = json!({"text": "Error: Cannot find module './validators/x'***\nmore"});
        assert_eq!(canonical_text(&recorded), canonical_text(&replayed));
        let with_frames = json!("TypeError: boom\n    at a (/x.js:1:2)\n    at b (/y.js:3:4)\n```");
        assert_eq!(
            canonical_text(&with_frames),
            canonical_text(&json!("TypeError: boom\n```"))
        );
    }

    #[test]
    fn canonical_text_ignores_key_order() {
        assert_eq!(
            canonical_text(&json!({"b": 1, "a": {"d": 2, "c": 3}})),
            canonical_text(&json!({"a": {"c": 3, "d": 2}, "b": 1}))
        );
    }

    #[test]
    fn mock_origin_in_error_texts_becomes_stable() -> Result<()> {
        let normalizer = RecordedValueNormalizer::new("http://127.0.0.1:53211")?;
        assert_eq!(
            normalizer.normalize_text("could not be parsed from http://127.0.0.1:53211/api/v3/repos/acme/app/contents/x.yml (invalid YAML)"),
            "could not be parsed from http://github.mock/api/v3/repos/acme/app/contents/x.yml (invalid YAML)"
        );
        Ok(())
    }
}
