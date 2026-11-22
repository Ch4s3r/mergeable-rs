use crate::error::{JsError, JsResult};
use crate::js;
use crate::validators::options::lib::{self, OptionResult};
use serde_json::Value;

const UNKNOWN_INPUT_TYPE_ERROR: &str = "Input type invalid, expected array of string as input";

static EMPTY_LIST: Value = Value::Array(Vec::new());

fn list_or_empty<'filter>(filter: Option<&'filter Value>, key: &str) -> JsResult<&'filter Value> {
    let listed = js::read(filter, key)?;
    Ok(match listed {
        Some(present) if js::is_truthy(present) => present,
        _ => &EMPTY_LIST,
    })
}

fn lowercase(user: &Value) -> JsResult<String> {
    match user {
        Value::String(text) => Ok(text.to_lowercase()),
        _ => Err(JsError::type_error("user.toLowerCase is not a function")),
    }
}

fn list_includes(list: &Value, list_name: &str, user: &Value) -> JsResult<bool> {
    match list {
        Value::Array(items) => Ok(items.iter().any(|item| js::strict_equals(item, user))),
        Value::String(text) => Ok(text.contains(&js::to_js_string(Some(user)))),
        _ => Err(JsError::type_error(format!(
            "{list_name}.includes is not a function"
        ))),
    }
}

pub fn process(validator_context: &Value, input: &Value, rule: &Value) -> JsResult<OptionResult> {
    let filter = js::get(rule, "required");
    let reviewers = list_or_empty(filter, "reviewers")?;
    let owners = list_or_empty(filter, "owners")?;
    let assignees = list_or_empty(filter, "assignees")?;
    let requested_reviewers = list_or_empty(filter, "requested_reviewers")?;
    let message = js::read(filter, "message")?;

    let Value::Array(approvers) = input else {
        return Err(JsError::error(UNKNOWN_INPUT_TYPE_ERROR));
    };
    let Value::Array(reviewer_list) = reviewers else {
        return Err(JsError::type_error("reviewers.map is not a function"));
    };
    let mut remaining_required: Vec<(String, &Value)> = Vec::new();
    for reviewer in reviewer_list {
        let lowercase_reviewer = lowercase(reviewer)?;
        match remaining_required
            .iter_mut()
            .find(|(key, _)| *key == lowercase_reviewer)
        {
            Some(entry) => entry.1 = reviewer,
            None => remaining_required.push((lowercase_reviewer, reviewer)),
        }
    }
    for approver in approvers {
        let lowercase_approver = lowercase(approver)?;
        remaining_required.retain(|(key, _)| *key != lowercase_approver);
    }
    let is_mergeable = remaining_required.is_empty();

    let mut required_descriptions = Vec::new();
    for &(_, user) in &remaining_required {
        let user_text = js::to_js_string(Some(user));
        let described = if list_includes(owners, "owners", user)? {
            format!("{user_text}(Code Owner) ")
        } else if list_includes(assignees, "assignees", user)? {
            format!("{user_text}(Assignee) ")
        } else if list_includes(requested_reviewers, "requestedReviewers", user)? {
            format!("{user_text}(Requested Reviewer) ")
        } else {
            format!("{user_text} ")
        };
        required_descriptions.push(described);
    }

    let validator_name = lib::validator_name(validator_context);
    let success_description = format!("{validator_name}: all required reviewers have approved");
    let failure_description = lib::message_or_default(
        message,
        format!(
            "{validator_name}: {}required",
            required_descriptions.join(",")
        ),
    );
    Ok(OptionResult::decided(
        is_mergeable,
        success_description,
        failure_description,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(rule: &Value, input: &Value) -> OptionResult {
        process(
            &json!({"name": "ValidatorName"}),
            input,
            &json!({"required": rule}),
        )
        .unwrap()
    }

    #[test]
    fn passes_when_every_required_reviewer_approved() {
        assert_eq!(
            run(
                &json!({"reviewers": ["shine2lay", "jusx"]}),
                &json!(["shine2lay", "jusx"])
            )
            .status,
            "pass"
        );
    }

    #[test]
    fn fails_when_a_required_reviewer_is_missing() {
        assert_eq!(
            run(
                &json!({"reviewers": ["shine2lay", "jusx"]}),
                &json!(["jusx"])
            )
            .status,
            "fail"
        );
    }

    #[test]
    fn passes_when_no_reviewers_are_required() {
        assert_eq!(
            run(&json!({"owners": true}), &json!(["jusx"])).status,
            "pass"
        );
    }

    #[test]
    fn reviewer_names_are_compared_case_insensitively() {
        assert_eq!(
            run(
                &json!({"reviewers": ["shine2lay", "jusxtest"]}),
                &json!(["jusxTest"])
            )
            .status,
            "fail"
        );
        let result = run(
            &json!({"reviewers": ["camelCaseUser", "jusxtest"]}),
            &json!(["jusxTest"]),
        );
        assert_eq!(
            result.description,
            json!("ValidatorName: camelCaseUser required")
        );
    }

    #[test]
    fn missing_reviewers_are_tagged_with_their_role() {
        let rule = json!({"reviewers": ["a", "b", "c", "d"], "owners": ["a"], "assignees": ["b"], "requested_reviewers": ["c"]});
        let result = run(&rule, &json!([]));
        assert_eq!(
            result.description,
            json!("ValidatorName: a(Code Owner) ,b(Assignee) ,c(Requested Reviewer) ,d required")
        );
    }
}
