use crate::context::Context;
use crate::error::JsResult;
use crate::validators::options_processor::list_processor;
use serde_json::{Value, json};

pub async fn process(
    context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> JsResult<Value> {
    let listed = rule.get("none_of").cloned().unwrap_or(Value::Null);
    let candidates = list_processor::process(&listed, context).await?;
    crate::validators::options::none_of::process(
        filter_context,
        input,
        &json!({"none_of": candidates}),
    )
    .map(|result| result.to_value())
}
