use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::github::api;
use crate::js;
use crate::lodash;
use crate::registry::Registry;
use crate::validators::Validator;
use async_trait::async_trait;
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde_json::{Map, Value, json};

const DAY_OF_THE_WEEK: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MAX_ISSUES: u64 = 20;
const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;

pub struct Stale;

pub struct LocalTime {
    pub day_of_week: usize,
    pub hour: u32,
}

pub fn local_time(now: DateTime<Utc>, time_zone: Option<&str>) -> LocalTime {
    let zoned = time_zone.and_then(|name| name.parse::<chrono_tz::Tz>().ok());
    match zoned {
        Some(zone) => {
            let local = now.with_timezone(&zone);
            LocalTime {
                day_of_week: local.weekday().num_days_from_sunday() as usize,
                hour: local.hour(),
            }
        }
        None => LocalTime {
            day_of_week: now.weekday().num_days_from_sunday() as usize,
            hour: now.hour(),
        },
    }
}

fn construct_stale_output(input: Value, settings: &Value, result: &Value) -> Value {
    let mut output = Map::new();
    output.insert("validator".to_string(), Value::String("stale".to_string()));
    if let Some(status) = js::get(result, "status") {
        output.insert("status".to_string(), status.clone());
    }
    if let Some(description) = js::get(result, "description") {
        output.insert("description".to_string(), description.clone());
    }
    output.insert(
        "details".to_string(),
        json!({"input": input, "settings": settings}),
    );
    Value::Object(output)
}

fn craft_fail_output(settings: &Value) -> Value {
    json!({
        "status": "fail",
        "name": "stale",
        "validations": construct_stale_output(Value::String("fail".to_string()), settings, settings),
    })
}

pub fn outside_time_constraint(settings: &Value, now: DateTime<Utc>) -> JsResult<bool> {
    let Some(time_options) = settings
        .get("time_constraint")
        .filter(|value| js::is_truthy(value))
    else {
        return Ok(false);
    };
    let time_zone = js::read(Some(time_options), "time_zone")?
        .filter(|value| js::is_truthy(value))
        .map(|zone| js::to_js_string(Some(zone)));
    let local = local_time(now, time_zone.as_deref());
    if let Some(days_of_week) =
        js::read(Some(time_options), "days_of_week")?.filter(|value| js::is_truthy(value))
    {
        let today = Value::String(DAY_OF_THE_WEEK[local.day_of_week].to_string());
        if !js::includes(Some(days_of_week), &today)? {
            return Ok(true);
        }
    }
    if let Some(hours_between) =
        js::read(Some(time_options), "hours_between")?.filter(|value| js::is_truthy(value))
    {
        let length = match hours_between {
            Value::Array(items) => Some(items.len()),
            Value::String(text) => Some(text.encode_utf16().count()),
            _ => None,
        };
        if length == Some(2) {
            let hour_now = json!(local.hour);
            let earliest = js::get(hours_between, "0");
            let latest = js::get(hours_between, "1");
            if lodash::less_than(Some(&hour_now), earliest)
                || lodash::greater_than(Some(&hour_now), latest)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn requested_types(settings: &Value) -> Vec<Value> {
    let type_setting = settings
        .get("type")
        .filter(|value| js::is_truthy(value))
        .cloned()
        .unwrap_or_else(|| json!(["issue", "pr"]));
    let types: Vec<Value> = match &type_setting {
        Value::Array(items) => items
            .iter()
            .filter(|item| item.as_str() == Some("issues") || item.as_str() == Some("pull_request"))
            .cloned()
            .collect(),
        other => vec![other.clone()],
    };
    types
        .iter()
        .map(|item| match item.as_str() {
            Some("issues") => Value::String("issue".to_string()),
            Some("pull_request") => Value::String("pr".to_string()),
            _ => Value::Null,
        })
        .collect()
}

pub fn search_timestamp(now: DateTime<Utc>, days: f64) -> JsResult<String> {
    let offset_milliseconds = days * MILLISECONDS_PER_DAY;
    let target_milliseconds = (now.timestamp_millis() as f64 - offset_milliseconds).trunc();
    if !target_milliseconds.is_finite() {
        return Err(JsError::named("RangeError", "Invalid time value"));
    }
    let target = DateTime::<Utc>::from_timestamp_millis(target_milliseconds as i64)
        .ok_or_else(|| JsError::named("RangeError", "Invalid time value"))?;
    Ok(target.format("%Y-%m-%dT%H:%M:%S").to_string())
}

fn label_query_parts(label: &Value, key: &str, prefix: &str) -> JsResult<Vec<String>> {
    match js::get(label, key).filter(|value| js::is_truthy(value)) {
        None => Ok(Vec::new()),
        Some(Value::Array(labels)) => Ok(labels
            .iter()
            .map(|name| format!("{prefix}label:\"{}\"", js::to_js_string(Some(name))))
            .collect()),
        Some(_) => Err(JsError::type_error(format!(
            "(label.{key} || []).map is not a function"
        ))),
    }
}

pub fn search_query(
    owner: &str,
    repository: &str,
    settings: &Value,
    types: &[Value],
    timestamp: &str,
) -> JsResult<String> {
    let type_query = if types.len() == 1 {
        format!(" type:{}", js::to_js_string(types.first()))
    } else {
        String::new()
    };
    let label = settings
        .get("label")
        .filter(|value| js::is_truthy(value))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut label_parts = label_query_parts(&label, "match", "")?;
    label_parts.extend(label_query_parts(&label, "ignore", "-")?);
    let label_query = label_parts.join(" ");
    let mut ignore_query = String::new();
    if js::truthy(settings.get("ignore_drafts")) {
        ignore_query.push_str("-is:draft ");
    }
    if js::truthy(settings.get("ignore_milestones")) {
        ignore_query.push_str("no:milestone ");
    }
    if js::truthy(settings.get("ignore_projects")) {
        ignore_query.push_str("no:project ");
    }
    let query = format!(
        "repo:{owner}/{repository} is:open updated:<{timestamp}{type_query} {label_query} {ignore_query}"
    );
    Ok(lodash::js_trim(&query).to_string())
}

pub fn schedule_result(items: &Value, input: Value, settings: &Value) -> JsResult<Value> {
    let items = match items {
        Value::Array(items) => items,
        Value::Null => {
            return Err(JsError::type_error(
                "Cannot read properties of null (reading 'filter')",
            ));
        }
        _ => return Err(JsError::type_error("items.filter is not a function")),
    };
    let mut issues = Vec::new();
    let mut pulls = Vec::new();
    for item in items {
        if item.is_null() {
            return Err(JsError::type_error(
                "Cannot read properties of null (reading 'pull_request')",
            ));
        }
        if js::truthy(js::get(item, "pull_request")) {
            pulls.push(item.clone());
        } else {
            issues.push(item.clone());
        }
    }
    let status = if !issues.is_empty() || !pulls.is_empty() {
        "pass"
    } else {
        "fail"
    };
    Ok(json!({
        "status": status,
        "name": "stale",
        "validations": construct_stale_output(Value::String(status.to_string()), &input, settings),
        "schedule": {"issues": issues, "pulls": pulls},
    }))
}

pub fn days_setting(settings: &Value) -> Value {
    settings
        .get("days")
        .filter(|value| js::is_truthy(value))
        .cloned()
        .unwrap_or_else(|| json!(20))
}

#[async_trait]
impl Validator for Stale {
    fn name(&self) -> &'static str {
        "stale"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["schedule.repository"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "days": "number",
            "type": "array",
            "label": {"match": "array", "ignore": "array"},
            "ignore_drafts": "boolean",
            "ignore_milestones": "boolean",
            "ignore_projects": "boolean",
            "time_constraint": {"time_zone": "string", "hours_between": "array", "days_of_week": "array"},
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let now = Utc::now();
        if outside_time_constraint(settings, now)? {
            return Ok(craft_fail_output(settings));
        }
        let days = days_setting(settings);
        let types = requested_types(settings);
        let timestamp = search_timestamp(Utc::now(), lodash::to_number(Some(&days)))?;
        let (owner, repository) = context.repo()?;
        let query = search_query(&owner, &repository, settings, &types, &timestamp)?;
        let items = api::search_issues_and_pull_requests(
            context,
            json!({"q": query, "sort": "updated", "order": "desc", "per_page": MAX_ISSUES}),
        )
        .await?;
        schedule_result(&items, json!({"days": days, "types": types}), settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn monday_eight_utc() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 15, 8, 30, 0)
            .single()
            .unwrap_or_default()
    }

    fn query_for(settings: Value) -> String {
        let types = requested_types(&settings);
        search_query("owner", "repo", &settings, &types, "2024-01-05T08:30:00").unwrap()
    }

    #[test]
    fn both_types_or_no_type_search_without_a_type_filter() {
        let both =
            query_for(json!({"do": "stale", "days": 10, "type": ["issues", "pull_request"]}));
        assert_eq!(both, "repo:owner/repo is:open updated:<2024-01-05T08:30:00");
        let default_types = query_for(json!({"do": "stale", "days": 10}));
        assert!(!default_types.contains("type:"));
    }

    #[test]
    fn single_type_adds_a_type_filter() {
        assert!(query_for(json!({"type": ["issues"]})).ends_with(" type:issue"));
        assert!(query_for(json!({"type": ["pull_request"]})).ends_with(" type:pr"));
        assert!(query_for(json!({"type": ["junk", "issues"]})).ends_with(" type:issue"));
        assert!(!query_for(json!({"type": ["junk1", "junk2"]})).contains("type:"));
    }

    #[test]
    fn label_match_and_ignore_become_label_qualifiers() {
        let query = query_for(
            json!({"label": {"match": ["test_match1", "test_match2"], "ignore": ["test_ignore"]}}),
        );
        assert_eq!(
            query,
            "repo:owner/repo is:open updated:<2024-01-05T08:30:00 label:\"test_match1\" label:\"test_match2\" -label:\"test_ignore\""
        );
        assert!(!query_for(json!({"days": 10})).contains("label:"));
    }

    #[test]
    fn ignore_flags_add_metadata_qualifiers() {
        for (draft, milestones, projects) in [
            (true, true, true),
            (false, true, false),
            (true, false, false),
            (false, false, true),
        ] {
            let query = query_for(
                json!({"ignore_drafts": draft, "ignore_milestones": milestones, "ignore_projects": projects}),
            );
            assert_eq!(query.contains("-is:draft"), draft);
            assert_eq!(query.contains("no:milestone"), milestones);
            assert_eq!(query.contains("no:project"), projects);
        }
    }

    #[test]
    fn issues_and_pulls_are_split_and_any_result_passes() {
        let result = schedule_result(
            &json!([{"number": 1}, {"number": 2, "pull_request": {}}]),
            json!({"days": 10, "types": []}),
            &json!({}),
        )
        .unwrap();
        assert_eq!(
            result["schedule"]["issues"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            result["schedule"]["pulls"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(result["status"], "pass");
        assert_eq!(
            result["validations"],
            json!({"validator": "stale", "details": {"input": "pass", "settings": {"days": 10, "types": []}}})
        );
    }

    #[test]
    fn nothing_stale_fails() {
        let result = schedule_result(
            &json!([]),
            json!({"days": 10, "types": ["issue"]}),
            &json!({}),
        )
        .unwrap();
        assert_eq!(result["status"], "fail");
    }

    #[test]
    fn days_of_week_constraint_only_runs_on_listed_days() {
        let sunday_only = json!({"time_constraint": {"days_of_week": ["Sun"]}});
        assert!(outside_time_constraint(&sunday_only, monday_eight_utc()).unwrap());
        let monday_only = json!({"time_constraint": {"days_of_week": ["Mon"]}});
        assert!(!outside_time_constraint(&monday_only, monday_eight_utc()).unwrap());
    }

    #[test]
    fn hours_between_constraint_compares_strings_as_numbers() {
        let inside = json!({"time_constraint": {"hours_between": ["7", "17"]}});
        assert!(!outside_time_constraint(&inside, monday_eight_utc()).unwrap());
        let too_early = json!({"time_constraint": {"hours_between": ["9", "17"]}});
        assert!(outside_time_constraint(&too_early, monday_eight_utc()).unwrap());
        let too_late = json!({"time_constraint": {"hours_between": ["1", "7"]}});
        assert!(outside_time_constraint(&too_late, monday_eight_utc()).unwrap());
    }

    #[test]
    fn time_zone_shifts_the_local_day_and_hour() {
        let late_sunday_utc = Utc
            .with_ymd_and_hms(2024, 1, 14, 23, 30, 0)
            .single()
            .unwrap_or_default();
        let tokyo = local_time(late_sunday_utc, Some("Asia/Tokyo"));
        assert_eq!((tokyo.day_of_week, tokyo.hour), (1, 8));
        let unknown = local_time(late_sunday_utc, Some("Not/AZone"));
        assert_eq!((unknown.day_of_week, unknown.hour), (0, 23));
    }

    #[test]
    fn fail_output_outside_the_time_window_echoes_the_settings() {
        let settings = json!({"do": "stale", "time_constraint": {"days_of_week": ["Sun"]}});
        let output = craft_fail_output(&settings);
        assert_eq!(
            output["validations"],
            json!({"validator": "stale", "details": {"input": "fail", "settings": settings}})
        );
    }

    #[test]
    fn search_timestamp_has_no_milliseconds_or_zone() {
        assert_eq!(
            search_timestamp(monday_eight_utc(), 10.0).unwrap(),
            "2024-01-05T08:30:00"
        );
    }

    #[test]
    fn days_default_to_twenty() {
        assert_eq!(days_setting(&json!({})), json!(20));
        assert_eq!(days_setting(&json!({"days": 0})), json!(20));
    }
}
