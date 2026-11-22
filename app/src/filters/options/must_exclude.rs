use crate::context::Context;
use crate::error::Result;
use crate::validators::options::lib::OptionResult;
use serde_json::Value;

pub fn process(
    _context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<Value> {
    crate::validators::options::must_exclude::process(filter_context, input, rule)
        .map(OptionResult::into_value)
}
