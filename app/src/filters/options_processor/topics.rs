use crate::context::Context;
use crate::error::Result;
use crate::filters::options;
use crate::filters::options_processor::name::ProcessedOption;
use crate::github::api;
use crate::json_value;
use serde_json::{Value, json};

pub async fn process(
    context: &Context,
    filter_context: &Value,
    settings: &Value,
) -> Result<ProcessedOption> {
    let input = repository_topics(context).await?;
    let topics_settings = settings.get("topics").unwrap_or(&Value::Null);
    let result = options::process(context, filter_context, &input, topics_settings).await?;
    Ok(ProcessedOption { input, result })
}

async fn repository_topics(context: &Context) -> Result<Value> {
    let cache_key = format!("{}/topics", context.repository_name()?);
    let use_config_cache = context.global_setting("use_config_cache")? == Some(&Value::Bool(true));
    if use_config_cache
        && let Some(names) = context.app.topics_cache.get(&cache_key).await
        && json_value::is_truthy(&names)
    {
        return Ok(names);
    }
    let params = json!({
        "owner": json_value::read_path(&context.payload, &["repository", "owner", "login"])?.unwrap_or(&Value::Null),
        "repo": json_value::read_path(&context.payload, &["repository", "name"])?.unwrap_or(&Value::Null),
    });
    let topics = api::fetch_all_topics(context, params).await?;
    if use_config_cache {
        context
            .app
            .topics_cache
            .set(&cache_key, topics.clone())
            .await;
    }
    Ok(topics)
}
