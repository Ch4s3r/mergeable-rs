use crate::context::Context;
use serde_json::{Value, json};

pub fn is_event_supported(supported_events: &[&str], event_name: &str) -> bool {
    let event_object = event_name.split('.').next().unwrap_or_default();
    let relevant: Vec<&&str> = supported_events
        .iter()
        .filter(|event| event.split('.').next() == Some(event_object) || **event == "*")
        .collect();
    let wildcard_for_object = format!("{event_object}.*");
    relevant
        .iter()
        .any(|event| **event == "*" || **event == wildcard_for_object || **event == event_name)
}

pub fn payload_key(context: &Context) -> &str {
    match context.event_name.as_str() {
        "issues" | "issue_comment" => "issue",
        "pull_request_review" => "pull_request",
        other => other,
    }
}

pub fn get_payload(context: &Context) -> Option<&Value> {
    context.payload.get(payload_key(context))
}

pub fn get_all_payload(context: &Context) -> &Value {
    &context.payload
}

pub fn get_event_attributes(context: &Context) -> Value {
    if context.event_name == "schedule" {
        return json!({"action": "", "repository": {}, "sender": {}});
    }
    let mut attributes = serde_json::Map::new();
    for key in ["action", "repository", "sender"] {
        if let Some(value) = context.payload.get(key) {
            attributes.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(attributes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_and_exact_events_are_supported() {
        assert!(is_event_supported(
            &["pull_request.*"],
            "pull_request.opened"
        ));
        assert!(is_event_supported(&["*"], "issues.opened"));
        assert!(is_event_supported(&["issues.opened"], "issues.opened"));
        assert!(!is_event_supported(&["issues.opened"], "issues.closed"));
        assert!(!is_event_supported(&["pull_request.*"], "issues.opened"));
    }
}
