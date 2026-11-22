use crate::error::{Error, Result};
use crate::json_value;
use crate::validators::options::output::{self, OptionResult};
use crate::validators::options::text_matching;
use serde_json::Value;

const REGEX_NOT_FOUND_ERROR: &str = "Failed to run the test because 'regex' is not provided for 'jira' option. Please check README for more information about configuration";
const SUCCESS_DESCRIPTION: &str = "The JIRA TICKET is valid";
const FAILURE_DESCRIPTION: &str = "The JIRA TICKET is not valid";

fn environment_or(name: &str, default_value: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => default_value.to_string(),
    }
}

fn non_empty_environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

#[must_use]
pub fn ticket_url(ticket_id: &str) -> Option<String> {
    let protocol = environment_or("JIRA_PROTOCOL", "https");
    let host = non_empty_environment("JIRA_HOST")?;
    let api_version = environment_or("JIRA_VERSION", "2");
    let host_part = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    let escaped_ticket = ticket_id.replace('#', "%23").replace('?', "%3F");
    Some(format!(
        "{}://{host_part}/rest/api/{api_version}/issue/{escaped_ticket}?expand=&fields=*all&properties=*all&fieldsByKeys=false",
        protocol.trim_end_matches(':')
    ))
}

fn has_error_messages(body: &Value) -> bool {
    matches!(body.get("errorMessages"), Some(Value::Array(messages)) if !messages.is_empty())
}

pub async fn check_ticket_status(ticket_id: &str) -> bool {
    let Some(url) = ticket_url(ticket_id) else {
        return false;
    };
    let Ok(client) = reqwest::Client::builder().build() else {
        return false;
    };
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json");
    if let (Some(username), Some(password)) = (
        non_empty_environment("JIRA_USERNAME"),
        non_empty_environment("JIRA_PASSWORD"),
    ) {
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
        Some(ticket) => check_ticket_status(&ticket).await,
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
        )
        .await
        .unwrap();
        assert_eq!(result.status, Status::Fail);
        assert_eq!(result.description, json!(FAILURE_DESCRIPTION));
    }

    #[tokio::test]
    async fn invalid_regex_names_the_pattern() {
        let error = process(&json!({}), &json!("x"), &json!({"jira": {"regex": "["}}))
            .await
            .unwrap_err();
        assert_eq!(
            error.message(),
            "Failed to create a regex expression with the provided regex: ["
        );
    }

    #[test]
    fn jira_errors_in_body_mean_invalid_ticket() {
        assert!(has_error_messages(
            &json!({"errorMessages": ["Issue does not exist"]})
        ));
        assert!(!has_error_messages(&json!({"errorMessages": []})));
        assert!(!has_error_messages(&json!({"key": "ABC-1"})));
    }
}
