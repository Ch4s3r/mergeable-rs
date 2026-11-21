use crate::annotations;
use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::js;
use crate::validators::options::lib::js_regex::JsRegex;
use crate::validators::options_processor::teams;
use futures::future::join_all;
use serde_json::Value;

const TEAM_PATTERN: &str = "^@.+\\/[^/]+$";

fn lowercase_member(member: &Value) -> JsResult<Value> {
    match member {
        Value::String(login) => Ok(Value::String(login.to_lowercase())),
        Value::Null => Err(JsError::type_error(
            "Cannot read properties of null (reading 'toLowerCase')",
        )),
        _ => Err(JsError::type_error("m.toLowerCase is not a function")),
    }
}

pub async fn process(list: &Value, context: &Context) -> JsResult<Vec<Value>> {
    let elements = js::as_array_or_wrap(list);
    let payload = event_aware::get_payload(context);
    let event = event_aware::get_event_attributes(context);
    let team_regex =
        JsRegex::new(TEAM_PATTERN, "").map_err(|_| JsError::error("invalid team pattern"))?;

    let mut candidates = Vec::new();
    let mut first_synchronous_error: Option<JsError> = None;
    let mut team_elements = Vec::new();
    for element in &elements {
        let element_text = match element {
            Value::String(text) => text,
            Value::Null => {
                first_synchronous_error.get_or_insert(JsError::type_error(
                    "Cannot read properties of null (reading 'match')",
                ));
                continue;
            }
            _ => {
                first_synchronous_error
                    .get_or_insert(JsError::type_error("element.match is not a function"));
                continue;
            }
        };
        if team_regex.test(element_text) {
            team_elements.push(element.clone());
            continue;
        }
        match annotations::search_and_replace_special_annotations(element_text, payload, &event) {
            Ok(replacement) => candidates.push(Value::String(replacement.to_lowercase())),
            Err(error) => {
                first_synchronous_error.get_or_insert(error);
            }
        }
    }

    let team_results = join_all(
        team_elements
            .iter()
            .map(|team| teams::extract_team_members(context, std::slice::from_ref(team))),
    )
    .await;
    if let Some(error) = first_synchronous_error {
        return Err(error);
    }
    for team_result in team_results {
        match team_result {
            Ok(members) => {
                for member in &members {
                    candidates.push(lowercase_member(member)?);
                }
            }
            Err(error) if error.is_named("TeamNotFoundError") => {}
            Err(error) => return Err(error),
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_slugs_are_recognised_by_their_at_and_slash() {
        let team_regex = JsRegex::new(TEAM_PATTERN, "").unwrap();
        assert!(team_regex.test("@org/team-slug"));
        assert!(!team_regex.test("@author"));
        assert!(!team_regex.test("org/team"));
        assert!(team_regex.test("@org/team/extra"));
    }
}
