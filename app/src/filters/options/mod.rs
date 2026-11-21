use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::js;
use serde_json::{Map, Value};

pub mod and;
pub mod boolean;
pub mod lib;
pub mod must_exclude;
pub mod must_include;
pub mod none_of;
pub mod one_of;
pub mod or;

async fn run_option(
    key: &str,
    context: &Context,
    filter_context: &Value,
    input: &Value,
    rule: &Value,
) -> JsResult<Value> {
    match key {
        "and" => and::process(context, filter_context, input, rule).await,
        "boolean" => boolean::process(context, filter_context, input, rule),
        "must_exclude" => must_exclude::process(context, filter_context, input, rule),
        "must_include" => must_include::process(context, filter_context, input, rule),
        "none_of" => none_of::process(context, filter_context, input, rule).await,
        "one_of" => one_of::process(context, filter_context, input, rule).await,
        "or" => or::process(context, filter_context, input, rule).await,
        unknown => Err(JsError::error(format!(
            "Cannot find module './options/{unknown}'"
        ))),
    }
}

fn is_option_unsupported(filter_context: &Value, key: &str) -> bool {
    let supported_options = filter_context.get("supportedOptions");
    js::truthy(supported_options)
        && !matches!(supported_options, Some(Value::Array(options)) if options.iter().any(|option| option.as_str() == Some(key)))
}

pub async fn process(
    context: &Context,
    filter_context: &Value,
    input: &Value,
    settings: &Value,
) -> JsResult<Value> {
    let mut output = Vec::new();
    for setting in js::as_array_or_wrap(settings) {
        for key in js::object_keys(Some(&setting))? {
            if key == "do" {
                continue;
            }
            let mut rule_fields = Map::new();
            if let Some(option_value) = js::get(&setting, &key) {
                rule_fields.insert(key.clone(), option_value.clone());
            }
            let rule = Value::Object(rule_fields);
            if is_option_unsupported(filter_context, &key) {
                let filter_name = js::to_js_string(filter_context.get("name"));
                output.push(lib::construct_error(
                    filter_context,
                    input,
                    &rule,
                    &format!("The '{key}' option is not supported for '{filter_name}' filter, please see README for all available options"),
                    None,
                ));
                continue;
            }
            match run_option(&key, context, filter_context, input, &rule).await {
                Ok(result) => output.push(lib::construct_output(
                    filter_context,
                    input,
                    &rule,
                    &result,
                    None,
                )),
                Err(error) => output.push(lib::construct_error(
                    filter_context,
                    input,
                    &rule,
                    &error.message,
                    None,
                )),
            }
        }
    }
    Ok(lib::consolidate_results(output, filter_context))
}
