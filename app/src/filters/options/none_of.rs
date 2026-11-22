use crate::context::Context;
use crate::error::Result;
use crate::validators::options::lib::OptionResult;
use crate::validators::options_processor::list_processor;
use serde_json::{Map, Value};

pub async fn process(
    context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<Value> {
    let listed = rule.get("none_of").unwrap_or(&Value::Null);
    let candidates = list_processor::process(listed, context).await?;
    let mut candidate_rule = Map::new();
    candidate_rule.insert("none_of".to_string(), Value::Array(candidates));
    crate::validators::options::none_of::process(
        filter_context,
        input,
        &Value::Object(candidate_rule),
    )
    .map(OptionResult::into_value)
}
