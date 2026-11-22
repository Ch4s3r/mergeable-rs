use crate::context::Context;
use crate::error::{Error, ErrorKind, Result};
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::registry::Registry;
use crate::results;
use crate::validators::Validator;
use crate::validators::options::lib::user_pattern::UserPattern;
use crate::validators::options_processor::deep_validation;
use async_trait::async_trait;
use futures::future::join_all;
use serde_json::{Value, json};

const MATCH_NOT_FOUND_ERROR: &str = "Failed to run the test because 'match' is not provided for 'project' option. Please check README for more information about configuration";
const DEFAULT_SUCCESS_MESSAGE: &str = "Required Project is present";
const ISSUE_URL_TEMPLATE_SUFFIX_LENGTH: usize = 9;

pub struct Project;

pub fn case_insensitive_regex(pattern: &str) -> Result<UserPattern> {
    UserPattern::new(pattern, "i").map_err(|_| {
        Error::new(
            ErrorKind::InvalidSyntax,
            format!("Invalid regular expression: /{pattern}/i: Invalid pattern"),
        )
    })
}

fn utf16_substring(units: &[u16], start: usize) -> String {
    String::from_utf16_lossy(units.get(start..).unwrap_or(&[]))
}

fn utf16_index_of(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub fn extract_issue_ids_from_cards(
    pull_request: Option<&Value>,
    cards: &[Option<Value>],
) -> Result<Vec<String>> {
    let issues_url = json_value::read(
        json_value::read(json_value::read(pull_request, "head")?, "repo")?,
        "issues_url",
    )?;
    let issues_url_units: Vec<u16> = match issues_url {
        Some(Value::String(url)) => url.encode_utf16().collect(),
        None => return Err(Error::missing_property("substring")),
        Some(Value::Null) => {
            return Err(Error::type_mismatch(
                "Cannot read properties of null (reading 'substring')",
            ));
        }
        Some(_) => return Err(Error::type_mismatch("issueUrl.substring is not a function")),
    };
    let prefix_length = issues_url_units
        .len()
        .saturating_sub(ISSUE_URL_TEMPLATE_SUFFIX_LENGTH);
    let issue_url_prefix = &issues_url_units[..prefix_length];
    let mut ids = Vec::new();
    for card in cards {
        let card_units: Vec<u16> = match card {
            Some(Value::String(url)) => url.encode_utf16().collect(),
            None => return Err(Error::missing_property("indexOf")),
            Some(Value::Null) => {
                return Err(Error::type_mismatch(
                    "Cannot read properties of null (reading 'indexOf')",
                ));
            }
            Some(_) => return Err(Error::type_mismatch("card.indexOf is not a function")),
        };
        if let Some(position) = utf16_index_of(&card_units, issue_url_prefix) {
            ids.push(utf16_substring(
                &card_units,
                position + issue_url_prefix.len() + 1,
            ));
        }
    }
    Ok(ids)
}

pub fn is_in_project(pull_request: Option<&Value>, card_urls: &[Option<Value>]) -> Result<bool> {
    let ids_from_cards = extract_issue_ids_from_cards(pull_request, card_urls)?;
    let pull_request_number =
        json_value::to_display_string(json_value::read(pull_request, "number")?);
    if ids_from_cards.contains(&pull_request_number) {
        return Ok(true);
    }
    let closed_issues =
        deep_validation::check_if_closes_an_issue(json_value::read(pull_request, "body")?);
    Ok(closed_issues
        .iter()
        .any(|issue| ids_from_cards.contains(issue)))
}

async fn project_card_urls(context: &Context, project_ids: &[Value]) -> Result<Vec<Option<Value>>> {
    let mut cards: Vec<Value> = Vec::new();
    for project_id in project_ids {
        let column_ids =
            api::project_list_columns(context, json!({"project_id": project_id})).await?;
        let responses =
            join_all(column_ids.iter().map(|column_id| {
                api::project_list_cards(context, json!({"column_id": column_id}))
            }))
            .await;
        for response in responses {
            match response?.data {
                Value::Array(column_cards) => cards.extend(column_cards),
                single_card => cards.push(single_card),
            }
        }
    }
    cards
        .into_iter()
        .map(|mut card| match card {
            Value::Null => Err(Error::type_mismatch(
                "Cannot read properties of null (reading 'content_url')",
            )),
            _ => Ok(json_value::get_mut(&mut card, "content_url").map(Value::take)),
        })
        .collect()
}

fn project_names(projects: Vec<Value>) -> Value {
    Value::Array(
        projects
            .into_iter()
            .map(|mut project| project.get_mut("name").map_or(Value::Null, Value::take))
            .collect(),
    )
}

#[async_trait]
impl Validator for Project {
    fn name(&self) -> &'static str {
        "project"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*", "issues.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "must_include": {"regex": "string", "regex_flag": "string", "message": "string"},
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let pull_request = event_aware::get_payload(context);
        let must_include = settings.get("must_include");
        let project_name = if json_value::truthy(must_include) {
            must_include.and_then(|option| json_value::get(option, "regex"))
        } else {
            must_include
        };
        let description = match settings
            .get("message")
            .filter(|message| json_value::is_truthy(message))
        {
            Some(message) => message.clone(),
            None => Value::String(format!(
                "Must be in the \"{}\" project.",
                json_value::to_display_string(project_name)
            )),
        };

        let projects = api::project_list_for_repo(context, context.repo_params(json!({}))?).await?;
        let validator_context = json!({"name": "project"});
        let Some(project_name) = project_name.filter(|name| json_value::is_truthy(name)) else {
            let output = results::construct_error_output(
                &Value::String("project".to_string()),
                Value::Array(projects),
                settings,
                MATCH_NOT_FOUND_ERROR,
                None,
            );
            return Ok(results::consolidate_result(
                vec![output],
                &validator_context,
            ));
        };

        let regex = case_insensitive_regex(&json_value::to_display_string(Some(project_name)))?;
        let matching_project_ids: Vec<Value> = projects
            .iter()
            .filter(|project| regex.is_match(&json_value::to_display_string(project.get("name"))))
            .map(|project| project.get("id").cloned().unwrap_or(Value::Null))
            .collect();
        let card_urls = project_card_urls(context, &matching_project_ids).await?;
        let is_mergeable = is_in_project(pull_request, &card_urls)?;

        let output = results::construct_output(
            &validator_context,
            project_names(projects),
            settings,
            if is_mergeable { "pass" } else { "fail" },
            if is_mergeable {
                Value::String(DEFAULT_SUCCESS_MESSAGE.to_string())
            } else {
                description
            },
            None,
        );
        Ok(results::consolidate_result(
            vec![output],
            &validator_context,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request(number: u64, body: &str) -> Value {
        json!({
            "number": number,
            "body": body,
            "head": {"repo": {"issues_url": "testRepo/issues{/number}"}},
        })
    }

    fn cards() -> Vec<Option<Value>> {
        vec![
            Some(json!("testRepo/issues/1")),
            Some(json!("testRepo/issues/2")),
        ]
    }

    #[test]
    fn issue_ids_are_taken_from_card_content_urls() {
        let ids = extract_issue_ids_from_cards(Some(&pull_request(1, "")), &cards()).unwrap();
        assert_eq!(ids, vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn pull_request_on_the_board_is_in_the_project() {
        assert!(is_in_project(Some(&pull_request(1, "")), &cards()).unwrap());
    }

    #[test]
    fn project_name_regex_ignores_case_and_uses_ascii_word_characters() {
        let regex = case_insensitive_regex(r"project\s\w+").unwrap();
        assert!(regex.is_match("PROJECT one"));
        assert!(!regex.is_match("Project éa"));
        assert!(
            case_insensitive_regex("Project One")
                .unwrap()
                .is_match("Project One")
        );
    }

    #[test]
    fn invalid_project_regex_is_a_syntax_error() {
        let error = case_insensitive_regex("(unclosed").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidSyntax);
    }

    #[test]
    fn card_without_content_url_fails_with_a_type_error() {
        let error = extract_issue_ids_from_cards(Some(&pull_request(1, "")), &[Some(Value::Null)])
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of null (reading 'indexOf')"
        );
    }
}
