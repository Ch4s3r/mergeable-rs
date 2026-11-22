use crate::context::Context;
use crate::error::Result;
use crate::validators::options::lib::OptionResult;
use serde_json::Value;

pub async fn process(
    _context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<Value> {
    crate::validators::options::and::process(filter_context, input, rule)
        .await
        .map(OptionResult::into_value)
}
