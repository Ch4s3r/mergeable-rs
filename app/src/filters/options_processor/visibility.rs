use crate::context::Context;
use crate::error::JsResult;
use crate::event_aware;
use crate::filters::options_processor::name::ProcessedOption;
use crate::js;
use serde_json::{Map, Value, json};

pub fn process(context: &Context, settings: &Value) -> JsResult<ProcessedOption> {
    let payload = event_aware::get_payload(context);
    let is_private = js::read(js::read(js::read(payload, "base")?, "repo")?, "private")?;
    let wanted_visibility = settings.get("visibility").and_then(Value::as_str);
    let mut status = "pass";
    if wanted_visibility == Some("public") && js::truthy(is_private) {
        status = "fail";
    }
    if wanted_visibility == Some("private") && !js::truthy(is_private) {
        status = "fail";
    }
    let mut input = Map::new();
    if let Some(private_flag) = is_private {
        input.insert("private".to_string(), private_flag.clone());
    }
    Ok(ProcessedOption {
        input: Value::Object(input),
        result: json!({"status": status}),
    })
}
