use crate::context::Context;
use serde_json::Value;
use std::sync::LazyLock;

static EMPTY_STRING: Value = Value::String(String::new());
static EMPTY_OBJECT: LazyLock<Value> = LazyLock::new(|| Value::Object(serde_json::Map::new()));

#[derive(Debug, Clone, Copy, Default)]
pub struct EventAttributes<'payload> {
    pub action: Option<&'payload Value>,
    pub repository: Option<&'payload Value>,
    pub sender: Option<&'payload Value>,
    pub app_name: Option<&'payload str>,
}

impl<'payload> EventAttributes<'payload> {
    #[must_use]
    pub fn from_payload(payload: &'payload Value) -> Self {
        Self {
            action: payload.get("action"),
            repository: payload.get("repository"),
            sender: payload.get("sender"),
            app_name: None,
        }
    }

    #[must_use]
    pub fn of_schedule() -> Self {
        Self {
            action: Some(&EMPTY_STRING),
            repository: Some(&EMPTY_OBJECT),
            sender: Some(&EMPTY_OBJECT),
            app_name: None,
        }
    }
}

#[must_use]
pub fn is_event_supported(supported_events: &[&str], event_name: &str) -> bool {
    let event_object = event_name.split('.').next().unwrap_or_default();
    supported_events.iter().any(|&event| {
        event == "*" || event == event_name || event.strip_suffix(".*") == Some(event_object)
    })
}

#[must_use]
pub fn payload_key(context: &Context) -> &str {
    match context.event_name.as_str() {
        "issues" | "issue_comment" => "issue",
        "pull_request_review" => "pull_request",
        other => other,
    }
}

#[must_use]
pub fn payload(context: &Context) -> Option<&Value> {
    context.payload.get(payload_key(context))
}

#[must_use]
pub fn event_attributes(context: &Context) -> EventAttributes<'_> {
    let attributes = if context.event_name == "schedule" {
        EventAttributes::of_schedule()
    } else {
        EventAttributes::from_payload(&context.payload)
    };
    EventAttributes {
        app_name: context.app.behavior.app_name.as_deref(),
        ..attributes
    }
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
        assert!(!is_event_supported(&["issues.opened.*"], "issues.opened"));
    }
}
