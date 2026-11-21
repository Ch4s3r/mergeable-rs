use crate::context::Context;
use crate::error::JsResult;
use serde_json::Value;

pub async fn process(
    _context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> JsResult<Value> {
    crate::validators::options::or::process(filter_context, input, rule)
        .await
        .map(|result| result.to_value())
}
