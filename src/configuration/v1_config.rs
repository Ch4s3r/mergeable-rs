use super::default_actions_for;
use crate::error::{Error, Result};
use crate::json_value;
use crate::results::Status;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

pub const DEFAULT_STALE_MESSAGE: &str = "There haven't been much activity here. This is stale. Is it still relevant? This is a friendly reminder to please resolve it. :-)";

static DEFAULT_ISSUES_FAIL_ACTIONS: LazyLock<Value> = LazyLock::new(|| {
    json!([{
        "do": "comment",
        "payload": {
            "body": "### We found some failed validations in your Issue\n{{#each validationSuites}}\n{{#ifEquals status \"fail\"}}\n#### {{{statusIcon status}}} Validator: {{toUpperCase name}}\n{{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***\nInput : {{{details.input}}}\nSettings : {{{displaySettings details.settings}}}\n{{/each}}\n{{/ifEquals}}\n{{/each}}"
        }
    }])
});

static DEFAULT_ISSUES_ERROR_ACTIONS: LazyLock<Value> = LazyLock::new(|| {
    json!([{
        "do": "comment",
        "payload": {
            "body": "### We found some error in your mergeable configuration\n{{#each validationSuites}}\n{{#ifEquals status \"error\"}}\n#### {{{statusIcon status}}} Validator: {{toUpperCase name}}\n{{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***\nInput : {{{details.input}}}\nSettings : {{{displaySettings details.settings}}}\n{{#if details.error}}\nError : {{{details.error}}}\n{{/if}}\n{{/each}}\n{{/ifEquals}}\n{{/each}}\n        "
        }
    }])
});

struct OutcomeActions {
    pass: Value,
    fail: Value,
    error: Value,
}

fn pull_request_outcomes() -> OutcomeActions {
    OutcomeActions {
        pass: default_actions_for(Status::Pass).clone(),
        fail: default_actions_for(Status::Fail).clone(),
        error: default_actions_for(Status::Error).clone(),
    }
}

fn issue_outcomes() -> OutcomeActions {
    OutcomeActions {
        pass: Value::Array(Vec::new()),
        fail: DEFAULT_ISSUES_FAIL_ACTIONS.clone(),
        error: DEFAULT_ISSUES_ERROR_ACTIONS.clone(),
    }
}

pub fn transform(config: &mut Value) -> Result<Value> {
    json_value::read(json_value::read(Some(config), "mergeable")?, "issues")?;
    let Some(mergeable) = json_value::get_mut(config, "mergeable") else {
        return Ok(json!({ "mergeable": [] }));
    };
    let mut output = Vec::new();
    let has_issues = json_value::truthy(json_value::get(mergeable, "issues"));
    let has_pull_requests = json_value::truthy(json_value::get(mergeable, "pull_requests"));
    if has_issues && let Some(issues) = json_value::get_mut(mergeable, "issues") {
        output.push(process_validators("issues.*", issues, issue_outcomes())?);
        process_stale("issues", issues, &mut output);
    }
    if has_pull_requests
        && let Some(pull_requests) = json_value::get_mut(mergeable, "pull_requests")
    {
        output.push(process_validators(
            "pull_request.*",
            pull_requests,
            pull_request_outcomes(),
        )?);
        output.push(process_validators(
            "pull_request_review.*",
            pull_requests,
            pull_request_outcomes(),
        )?);
        process_stale("pull_request", pull_requests, &mut output);
    }
    if !has_pull_requests && !has_issues {
        output.push(process_validators(
            "pull_request.*",
            mergeable,
            pull_request_outcomes(),
        )?);
        output.push(process_validators(
            "pull_request_review.*",
            mergeable,
            pull_request_outcomes(),
        )?);
    }
    Ok(json!({ "mergeable": output }))
}

fn process_stale(rule_type: &str, config: &Value, output: &mut Vec<Value>) {
    let stale = json_value::get(config, "stale");
    if !json_value::truthy(stale) {
        return;
    }
    let mut stale_validator = Map::new();
    stale_validator.insert("do".to_string(), json!("stale"));
    if let Some(days) = stale.and_then(|stale| json_value::get(stale, "days")) {
        stale_validator.insert("days".to_string(), days.clone());
    }
    stale_validator.insert("type".to_string(), json!(rule_type));
    let message = stale
        .and_then(|stale| json_value::get(stale, "message"))
        .filter(|message| json_value::is_truthy(message))
        .cloned()
        .unwrap_or_else(|| json!(DEFAULT_STALE_MESSAGE));
    output.push(json!({
        "when": "schedule.repository",
        "validate": [Value::Object(stale_validator)],
        "pass": [{ "do": "comment", "payload": { "body": message } }]
    }));
}

fn shorthand_validator(validator: &str, value: &Value) -> Result<Value> {
    match validator {
        "assignee" | "approvals" => Ok(json!({ "do": validator, "min": { "count": value } })),
        "label" | "title" => Ok(json!({ "do": validator, "must_exclude": { "regex": value } })),
        "milestone" | "project" => {
            Ok(json!({ "do": validator, "must_include": { "regex": value } }))
        }
        _ => Err(Error::type_mismatch(
            "simpleConfigMapping[validator] is not a function",
        )),
    }
}

fn is_array_index_key(key: &str) -> bool {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
    canonical
        && key.bytes().all(|byte| byte.is_ascii_digit())
        && key
            .parse::<u64>()
            .is_ok_and(|index| index < u64::from(u32::MAX))
}

fn keys_with_integer_keys_first(config: &Value) -> Result<Vec<String>> {
    let mut keys = json_value::object_keys(Some(config))?;
    if config.is_object() {
        let (mut index_keys, named_keys): (Vec<String>, Vec<String>) =
            keys.into_iter().partition(|key| is_array_index_key(key));
        index_keys.sort_by_key(|key| key.parse::<u64>().unwrap_or(u64::MAX));
        index_keys.extend(named_keys);
        keys = index_keys;
    }
    Ok(keys)
}

fn remove_excluded_keys(config: &mut Value) -> Result<()> {
    let exclude = json_value::get(config, "exclude");
    if !json_value::truthy(exclude) {
        return Ok(());
    }
    let Some(Value::String(exclude_list)) = exclude else {
        return Err(Error::type_mismatch(
            "config.exclude.split is not a function",
        ));
    };
    let mut excluded_keys: Vec<String> = exclude_list
        .split(',')
        .map(|item| {
            item.trim_matches(|character: char| {
                character.is_whitespace() || character == '\u{feff}'
            })
            .to_string()
        })
        .collect();
    excluded_keys.push("exclude".to_string());
    if let Value::Object(fields) = config {
        for key in excluded_keys {
            fields.shift_remove(&key);
        }
    }
    Ok(())
}

fn description_no_empty(value: &Value) -> Result<Option<Value>> {
    json_value::read(Some(value), "no_empty")?;
    if let Some(Value::Bool(enabled)) = json_value::get(value, "no_empty") {
        return Ok(Some(
            json!({ "do": "description", "no_empty": { "enabled": enabled } }),
        ));
    }
    let Some(no_empty_dashed) = json_value::get(value, "no-empty") else {
        return Ok(None);
    };
    let enabled = match json_value::read(Some(no_empty_dashed), "enabled")? {
        Some(enabled) if json_value::is_truthy(enabled) => enabled.clone(),
        _ => no_empty_dashed.clone(),
    };
    Ok(Some(
        json!({ "do": "description", "no_empty": { "enabled": enabled } }),
    ))
}

fn assign_do(validator: &str, value: &mut Value) -> Result<Value> {
    match value {
        Value::Null => Err(Error::type_mismatch(
            "Cannot convert undefined or null to object",
        )),
        Value::Object(fields) => {
            fields.insert("do".to_string(), json!(validator));
            Ok(value.clone())
        }
        // ponytail: a list cannot carry an extra `do` key in serde_json, so the list is kept as is
        _ => Ok(value.clone()),
    }
}

fn process_validators(event: &str, config: &mut Value, outcomes: OutcomeActions) -> Result<Value> {
    remove_excluded_keys(config)?;
    let mut validate = Vec::new();
    for validator in keys_with_integer_keys_first(config)? {
        if validator == "stale" {
            continue;
        }
        let Some(value) = json_value::get_mut(config, &validator) else {
            validate.push(shorthand_validator(&validator, &Value::Null)?);
            continue;
        };
        if json_value::type_of(Some(value)) != "object" {
            validate.push(shorthand_validator(&validator, value)?);
            continue;
        }
        if validator == "description"
            && let Some(description_rule) = description_no_empty(value)?
        {
            validate.push(description_rule);
            continue;
        }
        validate.push(assign_do(&validator, value)?);
    }
    let mut rule_set = Map::new();
    rule_set.insert("when".to_string(), json!(event));
    rule_set.insert("validate".to_string(), Value::Array(validate));
    rule_set.insert("pass".to_string(), outcomes.pass);
    rule_set.insert("fail".to_string(), outcomes.fail);
    rule_set.insert("error".to_string(), outcomes.error);
    Ok(Value::Object(rule_set))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transformed(mergeable: Value) -> Result<Value> {
        let mut config = Value::Object(Map::from_iter([("mergeable".to_string(), mergeable)]));
        transform(&mut config).map(|output| output["mergeable"].clone())
    }

    fn validate_of(rule_set: &Value) -> Value {
        rule_set["validate"].clone()
    }

    #[test]
    fn top_level_simple_values_become_pull_request_and_review_rule_sets() -> Result<()> {
        let rule_sets = transformed(json!({"title": "wip", "approvals": 2, "milestone": "v1"}))?;
        let expected_validate = json!([
            {"do": "title", "must_exclude": {"regex": "wip"}},
            {"do": "approvals", "min": {"count": 2}},
            {"do": "milestone", "must_include": {"regex": "v1"}}
        ]);
        assert_eq!(rule_sets[0]["when"], "pull_request.*");
        assert_eq!(rule_sets[1]["when"], "pull_request_review.*");
        assert_eq!(validate_of(&rule_sets[0]), expected_validate);
        assert_eq!(validate_of(&rule_sets[1]), expected_validate);
        assert_eq!(rule_sets[0]["pass"][0]["do"], "checks");
        Ok(())
    }

    #[test]
    fn issues_pull_requests_and_stale_produce_rule_sets_in_configuration_order() -> Result<()> {
        let rule_sets = transformed(json!({
            "issues": {"assignee": 1, "stale": {"days": 20}},
            "pull_requests": {"label": "wip", "stale": {"days": 10, "message": "ping"}}
        }))?;
        let events: Vec<&str> = rule_sets
            .as_array()
            .map(|sets| sets.iter().filter_map(|set| set["when"].as_str()).collect())
            .unwrap_or_default();
        assert_eq!(
            events,
            [
                "issues.*",
                "schedule.repository",
                "pull_request.*",
                "pull_request_review.*",
                "schedule.repository"
            ]
        );
        assert_eq!(rule_sets[0]["pass"], json!([]));
        assert_eq!(rule_sets[0]["fail"][0]["do"], "comment");
        assert_eq!(
            validate_of(&rule_sets[1]),
            json!([{"do": "stale", "days": 20, "type": "issues"}])
        );
        assert_eq!(
            rule_sets[1]["pass"][0]["payload"]["body"],
            DEFAULT_STALE_MESSAGE
        );
        assert_eq!(
            validate_of(&rule_sets[4]),
            json!([{"do": "stale", "days": 10, "type": "pull_request"}])
        );
        assert_eq!(rule_sets[4]["pass"][0]["payload"]["body"], "ping");
        Ok(())
    }

    #[test]
    fn exclude_list_removes_trimmed_keys_and_itself() -> Result<()> {
        let rule_sets = transformed(json!({
            "pull_requests": {"title": "wip", "label": "x", "stale": {"days": 3}, "exclude": "label, stale"}
        }))?;
        assert_eq!(rule_sets.as_array().map(Vec::len), Some(2));
        assert_eq!(
            validate_of(&rule_sets[1]),
            json!([{"do": "title", "must_exclude": {"regex": "wip"}}])
        );
        Ok(())
    }

    #[test]
    fn non_string_exclude_is_rejected() {
        let failure = transformed(json!({"exclude": 5}))
            .err()
            .map(|error| error.message().to_string());
        assert_eq!(
            failure.as_deref(),
            Some("config.exclude.split is not a function")
        );
    }

    #[test]
    fn description_no_empty_variants_become_enabled_flags() -> Result<()> {
        for (description, expected_enabled) in [
            (json!({"no_empty": true}), json!(true)),
            (json!({"no-empty": false}), json!(false)),
            (json!({"no-empty": {"enabled": true}}), json!(true)),
            (
                json!({"no-empty": {"enabled": false}}),
                json!({"enabled": false}),
            ),
        ] {
            let rule_sets = transformed(json!({ "description": description }))?;
            assert_eq!(
                validate_of(&rule_sets[0]),
                json!([{"do": "description", "no_empty": {"enabled": expected_enabled}}])
            );
        }
        Ok(())
    }

    #[test]
    fn object_validators_get_do_appended_or_kept_in_place() -> Result<()> {
        let rule_sets = transformed(json!({
            "description": {"must_include": {"regex": "a"}},
            "size": {"do": "ignored", "max": {"count": 5}}
        }))?;
        let validators = validate_of(&rule_sets[0]);
        let description_keys: Vec<&String> = validators[0]
            .as_object()
            .map(|fields| fields.keys().collect())
            .unwrap_or_default();
        assert_eq!(description_keys, ["must_include", "do"]);
        assert_eq!(validators[1], json!({"do": "size", "max": {"count": 5}}));
        Ok(())
    }

    #[test]
    fn unknown_shorthand_validator_is_rejected() {
        let failure = transformed(json!({"size": 5}))
            .err()
            .map(|error| error.message().to_string());
        assert_eq!(
            failure.as_deref(),
            Some("simpleConfigMapping[validator] is not a function")
        );
    }

    #[test]
    fn integer_like_keys_are_visited_first_in_ascending_order() -> Result<()> {
        let rule_sets = transformed(json!({"title": "a", "10": {}, "2": {}}))?;
        let order: Vec<Value> = validate_of(&rule_sets[0])
            .as_array()
            .map(|validators| {
                validators
                    .iter()
                    .map(|validator| validator["do"].clone())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(order, [json!("2"), json!("10"), json!("title")]);
        Ok(())
    }
}
