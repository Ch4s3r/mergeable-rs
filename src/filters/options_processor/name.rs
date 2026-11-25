use crate::context::Context;
use crate::error::Result;
use crate::filters::options;
use crate::json_value;
use serde_json::{Map, Value};

pub struct ProcessedOption {
    pub input: Value,
    pub result: Value,
}

pub async fn process(
    context: &Context,
    filter_context: &Value,
    settings: &Value,
) -> Result<ProcessedOption> {
    let repository_name = json_value::read_path(&context.payload, &["repository", "name"])?;
    let input = repository_name.unwrap_or(&Value::Null);
    let name_settings = settings.get("name").unwrap_or(&Value::Null);
    let result = options::process(context, filter_context, input, name_settings).await?;
    let mut described_input = Map::new();
    if let Some(name) = repository_name {
        described_input.insert("name".to_string(), name.clone());
    }
    Ok(ProcessedOption {
        input: Value::Object(described_input),
        result,
    })
}
