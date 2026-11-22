use crate::cache::ConfigCache;
use crate::context::Context;
use crate::error::JsResult;
use crate::filters::options;
use crate::filters::options_processor::name::ProcessedOption;
use crate::github::api;
use crate::js;
use serde_json::{Value, json};
use std::sync::LazyLock;

static TOPICS_CACHE: LazyLock<ConfigCache> = LazyLock::new(ConfigCache::from_env);

pub async fn process(
    context: &Context,
    filter_context: &Value,
    settings: &Value,
) -> JsResult<ProcessedOption> {
    let input = repository_topics(context).await?;
    let topics_settings = settings.get("topics").unwrap_or(&Value::Null);
    let result = options::process(context, filter_context, &input, topics_settings).await?;
    Ok(ProcessedOption { input, result })
}

async fn repository_topics(context: &Context) -> JsResult<Value> {
    let (owner, repo) = context.repo()?;
    let cache_key = format!("{owner}/{repo}/topics");
    let use_config_cache = context.global_setting("use_config_cache")? == Some(&Value::Bool(true));
    if use_config_cache
        && let Some(names) = TOPICS_CACHE.get(&cache_key).await
        && js::is_truthy(&names)
    {
        return Ok(names);
    }
    let params = json!({
        "owner": js::read_path(&context.payload, &["repository", "owner", "login"])?.cloned().unwrap_or(Value::Null),
        "repo": js::read_path(&context.payload, &["repository", "name"])?.cloned().unwrap_or(Value::Null),
    });
    let topics = api::get_all_topics(context, params).await?;
    if use_config_cache {
        TOPICS_CACHE.set(&cache_key, topics.clone()).await;
    }
    Ok(topics)
}
