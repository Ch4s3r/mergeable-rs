use crate::context::Context;
use crate::error::Result;
use crate::validators::options::output::OptionResult;
use serde_json::Value;

pub fn process(
    _context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<OptionResult> {
    crate::validators::options::must_exclude::process(filter_context, input, rule)
}
