use crate::app_config::JiraSettings;
use crate::error::{Error, Result};
use crate::github::client::encode_uri_component;
use crate::json_value;
use crate::validators::options::output::{self, OptionResult};
use crate::validators::options::text_matching;
use serde_json::Value;

const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'jira' option. Please check README for more information about configuration";
const SUCCESS_DESCRIPTION: &str = "The JIRA TICKET is valid";
const FAILURE_DESCRIPTION: &str = "The JIRA TICKET is not valid";

#[must_use]
pub fn ticket_url(ticket_id: &str, jira: &JiraSettings) -> Option<String> {
    let host = jira.host.as_deref()?;
    if matches!(ticket_id, "." | "..") {
        return None;
    }
    let host_part = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let escaped_ticket = encode_uri_component(ticket_id);
    Some(format!(
        "{}://{host_part}/rest/api/{}/issue/{escaped_ticket}?expand=&fields=*all&properties=*all&fieldsByKeys=false",
        jira.protocol.trim_end_matches(':'),
        jira.api_version
    ))
}

fn has_error_messages(body: &Value) -> bool {
    matches!(body.get("errorMessages"), Some(Value::Array(messages)) if !messages.is_empty())
}

pub async fn check_ticket_status(ticket_id: &str, jira: &JiraSettings) -> bool {
    let Some(url) = ticket_url(ticket_id, jira) else {
        return false;
    };
    let Ok(client) = reqwest::Client::builder().build() else {
        return false;
    };
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json");
    if let (Some(username), Some(password)) = (&jira.username, &jira.password) {
        request = request.basic_auth(username, Some(password));
    }
    let Ok(response) = request.send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let body_text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&body_text).unwrap_or(Value::Null);
    !has_error_messages(&body)
}

pub async fn process(
    _validator_context: &Value,
    input: &Value,
    rule: &Value,
    jira: &JiraSettings,
) -> Result<OptionResult> {
    let filter = json_value::get(rule, "jira");
    let message = json_value::read(filter, "message")?;
    let regex = json_value::read(filter, "regex")?;
    let Some(regex) = regex.filter(|present| json_value::is_truthy(present)) else {
        return Err(Error::general(REGEX_NOT_FOUND_ERROR));
    };
    let failure_description = output::message_or_default(message, FAILURE_DESCRIPTION.to_string());
    let flags = text_matching::regex_flags(filter.unwrap_or(&Value::Null));
    let ticket_id = {
        let compiled = text_matching::compile_user_regex(regex, &flags)?;
        compiled.first_match(&json_value::to_display_string(Some(input)))
    };
    let is_mergeable = match ticket_id {
        Some(ticket) => check_ticket_status(&ticket, jira).await,
        None => false,
    };
    Ok(OptionResult::decided(
        is_mergeable,
        SUCCESS_DESCRIPTION,
        failure_description,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::Status;
    use serde_json::json;

    #[tokio::test]
    async fn missing_regex_is_an_error() {
        let error = process(
            &json!({"name": "headRef"}),
            &json!("the test"),
            &json!({"jira": {}}),
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.message(), REGEX_NOT_FOUND_ERROR);
    }

    #[tokio::test]
    async fn input_without_ticket_fails_without_asking_jira() {
        let result = process(
            &json!({"name": "headRef"}),
            &json!("feature-123123"),
            &json!({"jira": {"regex": "test"}}),
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Fail);
        assert_eq!(result.description, json!(FAILURE_DESCRIPTION));
    }

    #[tokio::test]
    async fn invalid_regex_names_the_pattern() {
        let error = process(
            &json!({}),
            &json!("x"),
            &json!({"jira": {"regex": "["}}),
            &crate::app_config::JiraSettings::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Failed to create a regex expression with the provided regex: ["
        );
    }

    fn jira_on(host: &str) -> crate::app_config::JiraSettings {
        crate::app_config::JiraSettings::from_environment(
            &crate::app_config::Environment::from_iter([("JIRA_HOST", host)]),
        )
    }

    #[test]
    fn ticket_url_points_at_the_issue_of_the_ticket() {
        assert_eq!(
            ticket_url("ABC-123", &jira_on("jira.example.com")).as_deref(),
            Some(
                "https://jira.example.com/rest/api/2/issue/ABC-123?expand=&fields=*all&properties=*all&fieldsByKeys=false"
            )
        );
    }

    #[test]
    fn ticket_from_the_pull_request_cannot_leave_the_issue_path() {
        let jira = jira_on("jira.example.com");
        let traversal = ticket_url("../../../../admin?x=#y", &jira).unwrap_or_default();
        assert!(traversal.starts_with(
            "https://jira.example.com/rest/api/2/issue/..%2F..%2F..%2F..%2Fadmin%3Fx%3D%23y?"
        ));
        assert_eq!(ticket_url("..", &jira), None);
        assert_eq!(ticket_url(".", &jira), None);
    }

    #[test]
    fn jira_errors_in_body_mean_invalid_ticket() {
        assert!(has_error_messages(
            &json!({"errorMessages": ["Issue does not exist"]})
        ));
        assert!(!has_error_messages(&json!({"errorMessages": []})));
        assert!(!has_error_messages(&json!({"key": "ABC-1"})));
    }

    fn head_ref_validator_context() -> Value {
        json!({"name": "headRef", "supportedOptions": ["and", "or", "begins_with", "ends_with", "max", "min", "must_exclude", "must_include", "no_empty", "required", "jira"]})
    }

    fn unreachable_jira() -> JiraSettings {
        JiraSettings {
            protocol: "http".to_string(),
            host: Some("127.0.0.1".to_string()),
            api_version: "2".to_string(),
            username: Some("user".to_string()),
            password: Some("password".to_string()),
        }
    }

    fn unconfigured_jira() -> JiraSettings {
        JiraSettings {
            host: None,
            ..unreachable_jira()
        }
    }

    #[tokio::test]
    async fn return_false_with_wrong_ticket_with_find_issue() {
        assert!(!check_ticket_status("wrong-ticket-number", &unreachable_jira()).await);
    }

    #[tokio::test]
    async fn return_error_if_inputs_are_not_in_expected_format() {
        let error = process(
            &head_ref_validator_context(),
            &json!("the test"),
            &json!({"jira": {}}),
            &unreachable_jira(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Failed to run the test because 'regex' is not provided for 'jira' option. Please check README for more information about configuration"
        );
    }

    #[tokio::test]
    async fn return_fail_if_input_does_not_meet_the_regex_criteria() {
        let result = process(
            &head_ref_validator_context(),
            &json!("feature-123123"),
            &json!({"jira": {"regex": "test"}}),
            &unreachable_jira(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Fail);
    }

    #[tokio::test]
    async fn return_fail_if_jira_ticket_id_could_not_be_found() {
        let result = process(
            &head_ref_validator_context(),
            &json!("test-123123"),
            &json!({"jira": {"regex": "test"}}),
            &unreachable_jira(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Fail);
    }

    #[tokio::test]
    async fn return_fail_if_jira_ticket_id_could_not_be_found_without_a_configured_jira_host() {
        let result = process(
            &head_ref_validator_context(),
            &json!("test-123123"),
            &json!({"jira": {"regex": "test"}}),
            &unconfigured_jira(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Fail);
    }
}
