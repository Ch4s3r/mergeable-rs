use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::filters::options_processor::name::ProcessedOption;
use crate::json_value;
use crate::results::Status;
use serde_json::{Map, Value, json};

pub fn process(context: &Context, settings: &Value) -> Result<ProcessedOption> {
    let payload = event_aware::get_payload(context);
    let is_private = json_value::read(
        json_value::read(json_value::read(payload, "base")?, "repo")?,
        "private",
    )?;
    let wanted_visibility = settings.get("visibility").and_then(Value::as_str);
    let repository_is_private = json_value::truthy(is_private);
    let status = Status::from_check(match wanted_visibility {
        Some("public") => !repository_is_private,
        Some("private") => repository_is_private,
        _ => true,
    });
    let mut input = Map::new();
    if let Some(private_flag) = is_private {
        input.insert("private".to_string(), private_flag.clone());
    }
    Ok(ProcessedOption {
        input: Value::Object(input),
        result: json!({"status": status.as_str()}),
    })
}
