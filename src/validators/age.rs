use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::json_value;
use crate::registry::Registry;
use crate::results::{self, Status};
use crate::validators::Validator;
use async_trait::async_trait;
use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::fmt::{rfc2822, strtime};
use jiff::tz::TimeZone;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;
const LOCAL_DATE_TIME_FORMATS: [&str; 4] = [
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%dT%H:%M",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%d %H:%M",
];

static RFC_3339_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}[Tt ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})$").ok()
});

pub struct Age;

#[must_use]
pub fn compared_time_milliseconds(time_to_compare: Option<&Value>, now: Timestamp) -> f64 {
    match time_to_compare {
        None => json_value::number_from_timestamp_milliseconds(now.as_millisecond()),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => parse_date_text(text).unwrap_or(f64::NAN),
        Some(_) => f64::NAN,
    }
}

fn parse_date_text(text: &str) -> Option<f64> {
    parse_instant(text)
        .map(|instant| json_value::number_from_timestamp_milliseconds(instant.as_millisecond()))
}

fn parse_instant(text: &str) -> Option<Timestamp> {
    if is_rfc_3339(text) {
        return text.parse::<Timestamp>().ok();
    }
    if let Ok(date) = strtime::parse("%Y-%m-%d", text).and_then(|parsed| parsed.to_date()) {
        return local_instant(date.to_datetime(jiff::civil::Time::midnight()));
    }
    for format in LOCAL_DATE_TIME_FORMATS {
        if let Ok(date_time) = strtime::parse(format, text).and_then(|parsed| parsed.to_datetime())
        {
            return local_instant(date_time);
        }
    }
    rfc2822::DateTimeParser::new().parse_timestamp(text).ok()
}

fn is_rfc_3339(text: &str) -> bool {
    RFC_3339_PATTERN
        .as_ref()
        .is_some_and(|pattern| pattern.is_match(text))
}

fn local_instant(date_time: DateTime) -> Option<Timestamp> {
    TimeZone::system()
        .to_ambiguous_timestamp(date_time)
        .unambiguous()
        .ok()
}

#[must_use]
pub fn check_age(now: Timestamp, time_to_compare: Option<&Value>, settings: &Value) -> Value {
    let elapsed_milliseconds = json_value::number_from_timestamp_milliseconds(now.as_millisecond())
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

pub fn evaluate(now: Timestamp, payload: Option<&Value>, settings: &Value) -> Result<Value> {
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
        evaluate(Timestamp::now(), event_aware::payload(context), settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::SignedDuration;

    fn iso(time: Timestamp) -> Value {
        Value::String(format!("{time:.3}"))
    }

    fn hours(count: i64) -> SignedDuration {
        SignedDuration::from_hours(count)
    }

    fn days(count: i64) -> SignedDuration {
        SignedDuration::from_hours(count * 24)
    }

    #[test]
    fn date_formats_accepted_by_the_age_validator() {
        let utc = |text: &str| parse_instant(text).map(Timestamp::as_millisecond);
        assert_eq!(utc("2024-01-15T10:00:00Z"), Some(1_705_312_800_000));
        assert_eq!(utc("2024-01-15t10:00:00.5z"), Some(1_705_312_800_500));
        assert_eq!(utc("2024-01-15 11:00:00+01:00"), Some(1_705_312_800_000));
        assert_eq!(
            utc("Mon, 15 Jan 2024 10:00:00 GMT"),
            Some(1_705_312_800_000)
        );
        for local_text in [
            "2024-01-15",
            "2024-01-15T10:00:00.123",
            "2024-01-15T10:00",
            "2024-01-15 10:00:00",
            "2024-01-15 10:00",
        ] {
            assert!(parse_instant(local_text).is_some(), "{local_text}");
        }
        for invalid_text in ["2024-01-15T10:00Z", "20240115T100000Z", "not a date", ""] {
            assert!(parse_instant(invalid_text).is_none(), "{invalid_text}");
        }
    }

    #[test]
    fn pull_request_created_an_hour_ago_is_not_one_day_old() {
        let now = Timestamp::now();
        let payload = json!({"created_at": iso(now - hours(1))});
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
        let now = Timestamp::now();
        let payload = json!({"created_at": iso(now - days(2))});
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
        let now = Timestamp::now();
        let settings = json!({"do": "age", "updated_at": {"days": 1}});
        let recently_updated =
            json!({"created_at": iso(now - days(3)), "updated_at": iso(now - hours(1))});
        assert_eq!(
            evaluate(now, Some(&recently_updated), &settings).unwrap()["status"],
            "fail"
        );
        let updated_long_ago =
            json!({"created_at": iso(now - days(3)), "updated_at": iso(now - days(2))});
        assert_eq!(
            evaluate(now, Some(&updated_long_ago), &settings).unwrap()["status"],
            "pass"
        );
    }

    #[test]
    fn custom_message_is_used_when_too_young() {
        let now = Timestamp::now();
        let payload = json!({"created_at": iso(now - hours(1))});
        let settings = json!({"do": "age", "created_at": {"days": 1, "message": "You need to wait at least one day before you can merge the PR"}});
        let result = evaluate(now, Some(&payload), &settings).unwrap();
        assert_eq!(
            result["validations"][0]["description"],
            "You need to wait at least one day before you can merge the PR"
        );
    }

    #[test]
    fn unparsable_date_never_counts_as_old_enough() {
        let now = Timestamp::now();
        let payload = json!({"created_at": "not a date"});
        let result = evaluate(now, Some(&payload), &json!({"created_at": {"days": 0}})).unwrap();
        assert_eq!(result["status"], "fail");
    }

    #[test]
    fn missing_date_is_treated_as_now() {
        let now = Timestamp::now();
        let result = evaluate(now, Some(&json!({})), &json!({"created_at": {"days": 0}})).unwrap();
        assert_eq!(result["status"], "pass");
        assert!(result["validations"][0]["details"].get("input").is_none());
    }
}
