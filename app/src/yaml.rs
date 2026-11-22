use serde_json::{Map, Number, Value};

#[derive(Debug, Clone)]
pub struct YamlError {
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

fn contains_tag(value: &serde_yaml_ng::Value) -> bool {
    match value {
        serde_yaml_ng::Value::Tagged(_) => true,
        serde_yaml_ng::Value::Sequence(items) => items.iter().any(contains_tag),
        serde_yaml_ng::Value::Mapping(mapping) => mapping
            .iter()
            .any(|(key, item)| contains_tag(key) || contains_tag(item)),
        _ => false,
    }
}

pub fn parse(text: &str) -> Result<Option<Value>, YamlError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut parsed: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(text).map_err(|error| YamlError {
            message: error.to_string(),
        })?;
    if contains_tag(&parsed) {
        return Err(YamlError {
            message: "unknown tag".to_string(),
        });
    }
    parsed.apply_merge().map_err(|error| YamlError {
        message: error.to_string(),
    })?;
    Ok(Some(to_json(parsed)))
}

fn to_json(value: serde_yaml_ng::Value) -> Value {
    match value {
        serde_yaml_ng::Value::Null => Value::Null,
        serde_yaml_ng::Value::Bool(flag) => Value::Bool(flag),
        serde_yaml_ng::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Value::Number(integer.into())
            } else if let Some(unsigned) = number.as_u64() {
                Value::Number(unsigned.into())
            } else {
                match number.as_f64() {
                    Some(float) if float.fract() == 0.0 && float.abs() < 9.0e15 => {
                        Value::Number((float as i64).into())
                    }
                    Some(float) => Number::from_f64(float).map_or(Value::Null, Value::Number),
                    None => Value::Null,
                }
            }
        }
        serde_yaml_ng::Value::String(text) => Value::String(text),
        serde_yaml_ng::Value::Sequence(items) => {
            Value::Array(items.into_iter().map(to_json).collect())
        }
        serde_yaml_ng::Value::Mapping(mapping) => {
            let mut object = Map::with_capacity(mapping.len());
            for (key, item) in mapping {
                let key_text = match to_json(key) {
                    Value::String(text) => text,
                    other => crate::js::to_js_string(Some(&other)),
                };
                object.insert(key_text, to_json(item));
            }
            Value::Object(object)
        }
        serde_yaml_ng::Value::Tagged(tagged) => to_json(tagged.value),
    }
}
