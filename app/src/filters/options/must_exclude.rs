use crate::context::Context;
use crate::error::JsResult;
use serde_json::Value;

pub fn process(
    _context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> JsResult<Value> {
    crate::validators::options::must_exclude::process(filter_context, input, rule)
        .map(|result| result.to_value())
}
