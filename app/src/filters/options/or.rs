use crate::context::Context;
use crate::error::Result;
use crate::validators::options::output::OptionResult;
use serde_json::Value;

pub async fn process(
    context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> Result<OptionResult> {
    crate::validators::options::or::process(filter_context, input, rule, &context.app.behavior.jira)
        .await
}
