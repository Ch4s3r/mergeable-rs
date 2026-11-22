use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::results::{self, Status};
use crate::validators::Validator;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{Map, Value, json};

const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;

pub struct Age;

#[must_use]
pub fn compared_time_milliseconds(time_to_compare: Option<&Value>, now: DateTime<Utc>) -> f64 {
    match time_to_compare {
        None => json_value::number_from_timestamp_milliseconds(now.timestamp_millis()),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => parse_date_text(text).unwrap_or(f64::NAN),
        Some(_) => f64::NAN,
    }
}

fn parse_date_text(text: &str) -> Option<f64> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(json_value::number_from_timestamp_milliseconds(
            parsed.timestamp_millis(),
        ));
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .map(|midnight| chrono::Local.from_local_datetime(&midnight))
            .and_then(chrono::offset::LocalResult::single)
            .map(|local| json_value::number_from_timestamp_milliseconds(local.timestamp_millis()));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return chrono::Local
                .from_local_datetime(&naive)
                .single()
                .map(|local| {
                    json_value::number_from_timestamp_milliseconds(local.timestamp_millis())
                });
        }
    }
    DateTime::parse_from_rfc2822(text)
        .ok()
        .map(|parsed| json_value::number_from_timestamp_milliseconds(parsed.timestamp_millis()))
}

#[must_use]
pub fn check_age(now: DateTime<Utc>, time_to_compare: Option<&Value>, settings: &Value) -> Value {
    let elapsed_milliseconds =
        json_value::number_from_timestamp_milliseconds(now.timestamp_millis())
            - compared_time_milliseconds(time_to_compare, now);
    let elapsed_days = elapsed_milliseconds / MILLISECONDS_PER_DAY;
    let is_mergeable = elapsed_days >= json_value::to_number(settings.get("days"));
    let description = if is_mergeable {
        Value::String("Your PR is old enough to merge".to_string())
    } else {
        match settings
            .get("message")
            .filter(|message| json_value::is_truthy(message))
        {
            Some(message) => message.clone(),
            None => Value::String("Your PR is not old enough to be merged yet".to_string()),
        }
    };
    let mut details = Map::new();
    if let Some(input) = time_to_compare {
        details.insert("input".to_string(), input.clone());
    }
    details.insert("settings".to_string(), settings.clone());
    json!({
        "validator": {"name": "age"},
        "status": Status::from_check(is_mergeable).as_str(),
        "description": description,
        "details": Value::Object(details),
    })
}

pub fn evaluate(now: DateTime<Utc>, payload: Option<&Value>, settings: &Value) -> Result<Value> {
    let mut output = Vec::new();
    for timestamp_field in ["created_at", "updated_at"] {
        if let Some(age_settings) = settings
            .get(timestamp_field)
            .filter(|value| json_value::is_truthy(value))
        {
            output.push(check_age(
                now,
                json_value::read(payload, timestamp_field)?,
                age_settings,
            ));
        }
    }
    Ok(results::consolidate_result(output, &json!({"name": "age"})))
}

#[async_trait]
impl Validator for Age {
    fn name(&self) -> &'static str {
        "time"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*", "issue_comment.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "updated_at": {"days": "number", "message": "string"},
            "created_at": {"days": "number", "message": "string"},
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        evaluate(Utc::now(), event_aware::payload(context), settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn iso(time: DateTime<Utc>) -> Value {
        Value::String(time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    }

    #[test]
    fn pull_request_created_an_hour_ago_is_not_one_day_old() {
        let now = Utc::now();
        let payload = json!({"created_at": iso(now - Duration::hours(1))});
        let result = evaluate(
            now,
            Some(&payload),
            &json!({"do": "age", "created_at": {"days": 1}}),
        )
        .unwrap();
        assert_eq!(result["status"], "fail");
    }

    #[test]
    fn pull_request_created_two_days_ago_passes_one_day_minimum() {
        let now = Utc::now();
        let payload = json!({"created_at": iso(now - Duration::days(2))});
        let result = evaluate(
            now,
            Some(&payload),
            &json!({"do": "age", "created_at": {"days": 1}}),
        )
        .unwrap();
        assert_eq!(result["status"], "pass");
        assert_eq!(
            result["validations"][0]["description"],
            "Your PR is old enough to merge"
        );
    }

    #[test]
    fn updated_at_option_uses_the_last_update_time() {
        let now = Utc::now();
        let settings = json!({"do": "age", "updated_at": {"days": 1}});
        let recently_updated = json!({"created_at": iso(now - Duration::days(3)), "updated_at": iso(now - Duration::hours(1))});
        assert_eq!(
            evaluate(now, Some(&recently_updated), &settings).unwrap()["status"],
            "fail"
        );
        let updated_long_ago = json!({"created_at": iso(now - Duration::days(3)), "updated_at": iso(now - Duration::days(2))});
        assert_eq!(
            evaluate(now, Some(&updated_long_ago), &settings).unwrap()["status"],
            "pass"
        );
    }

    #[test]
    fn custom_message_is_used_when_too_young() {
        let now = Utc::now();
        let payload = json!({"created_at": iso(now - Duration::hours(1))});
        let settings = json!({"do": "age", "created_at": {"days": 1, "message": "You need to wait at least one day before you can merge the PR"}});
        let result = evaluate(now, Some(&payload), &settings).unwrap();
        assert_eq!(
            result["validations"][0]["description"],
            "You need to wait at least one day before you can merge the PR"
        );
    }

    #[test]
    fn unparsable_date_never_counts_as_old_enough() {
        let now = Utc::now();
        let payload = json!({"created_at": "not a date"});
        let result = evaluate(now, Some(&payload), &json!({"created_at": {"days": 0}})).unwrap();
        assert_eq!(result["status"], "fail");
    }

    #[test]
    fn missing_date_is_treated_as_now() {
        let now = Utc::now();
        let result = evaluate(now, Some(&json!({})), &json!({"created_at": {"days": 0}})).unwrap();
        assert_eq!(result["status"], "pass");
        assert!(result["validations"][0]["details"].get("input").is_none());
    }
}
