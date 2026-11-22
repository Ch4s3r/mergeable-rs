use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::github::api;
use crate::js;
use crate::results;
use crate::validators::options_processor::unique_values;
use serde_json::{Map, Value};

fn split_team(team: &Value) -> JsResult<(String, String)> {
    let team_text = match team {
        Value::String(text) => text,
        Value::Null => {
            return Err(JsError::type_error(
                "Cannot read properties of null (reading 'split')",
            ));
        }
        _ => return Err(JsError::type_error("team.split is not a function")),
    };
    let parts: Vec<&str> = team_text.split('/').collect();
    if parts.len() != 2 {
        return Err(JsError::error(
            "each team id needs to be given in 'org/team_slug'",
        ));
    }
    let organization = parts[0].strip_prefix('@').unwrap_or(parts[0]);
    Ok((organization.to_string(), parts[1].to_string()))
}

pub async fn extract_team_members(context: &Context, teams: &[Value]) -> JsResult<Vec<Value>> {
    let mut team_members = Vec::new();
    for team in teams {
        let (organization, team_slug) = split_team(team)?;
        team_members.extend(api::list_members_in_org(context, &organization, &team_slug).await?);
    }
    Ok(unique_values(team_members))
}

pub async fn extract_team_memberships(
    context: &Context,
    teams: &[Value],
    users: &[Value],
) -> JsResult<Vec<Value>> {
    let mut team_members = Vec::new();
    if teams.is_empty() {
        return Ok(team_members);
    }
    for user in users {
        for team in teams {
            let (organization, team_slug) = split_team(team)?;
            if api::get_membership_for_user_in_org(context, &organization, &team_slug, user).await?
            {
                team_members.push(user.clone());
            }
        }
    }
    Ok(unique_values(team_members))
}

#[must_use]
pub fn context_as_validator(context: &Context) -> Value {
    let mut validator = Map::new();
    validator.insert(
        "eventId".to_string(),
        Value::String(context.event_id.clone()),
    );
    validator.insert(
        "eventName".to_string(),
        Value::String(context.event_name.clone()),
    );
    validator.insert("payload".to_string(), Value::clone(&context.payload));
    Value::Object(validator)
}

#[must_use]
pub fn team_option_output(
    validator_context: Value,
    team_memberships: Vec<Value>,
    settings: &Value,
    team_name: &Value,
    user_name: &Value,
) -> Value {
    let is_member = team_memberships
        .iter()
        .any(|member| js::strict_equals(member, user_name));
    let user_text = js::to_js_string(Some(user_name));
    let team_text = js::to_js_string(Some(team_name));
    let (status, description) = if is_member {
        (
            "pass",
            format!("'{user_text}' is part of the '{team_text}' team'"),
        )
    } else {
        (
            "fail",
            format!("'{user_text}' is not part of the '{team_text}' team'"),
        )
    };
    let mut consolidation_context = Map::new();
    if let Some(name) = js::get(&validator_context, "name") {
        consolidation_context.insert("name".to_string(), name.clone());
    }
    let output = vec![results::construct_output_with_owned_validator(
        validator_context,
        Value::Array(team_memberships),
        settings,
        status,
        Value::String(description),
        Some(Value::Null),
    )];
    results::consolidate_result(output, &Value::Object(consolidation_context))
}

pub async fn process_team_option(
    context: &Context,
    settings: &Value,
    payload: Option<&Value>,
) -> JsResult<Value> {
    let team_name = js::get(settings, "team").unwrap_or(&Value::Null);
    let user_name = js::read(js::read(payload, "user")?, "login")?.unwrap_or(&Value::Null);
    let team_memberships = extract_team_memberships(
        context,
        std::slice::from_ref(team_name),
        std::slice::from_ref(user_name),
    )
    .await?;
    // ponytail: Node puts the whole mergeable Context object into the output's validator field; only its serialisable event fields are kept here
    Ok(team_option_output(
        context_as_validator(context),
        team_memberships,
        settings,
        team_name,
        user_name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn team_id_must_have_organization_and_slug() {
        assert_eq!(
            split_team(&json!("@org/team1")).unwrap(),
            ("org".to_string(), "team1".to_string())
        );
        assert_eq!(
            split_team(&json!("org/team1")).unwrap(),
            ("org".to_string(), "team1".to_string())
        );
        assert_eq!(
            split_team(&json!("team1")).unwrap_err().message,
            "each team id needs to be given in 'org/team_slug'"
        );
    }

    #[test]
    fn team_option_passes_for_members_and_has_no_validator_name() {
        let settings = json!({"do": "author", "team": "org/team-slug"});
        let result = team_option_output(
            json!({"eventName": "pull_request"}),
            vec![json!("alice")],
            &settings,
            &json!("org/team-slug"),
            &json!("alice"),
        );
        assert_eq!(result["status"], "pass");
        assert!(result.get("name").is_none());
        assert_eq!(
            result["validations"][0]["description"],
            "'alice' is part of the 'org/team-slug' team'"
        );
        assert_eq!(result["validations"][0]["details"]["error"], Value::Null);
    }

    #[test]
    fn team_option_fails_for_non_members() {
        let settings = json!({"do": "author", "team": "org/team-slug"});
        let result = team_option_output(
            json!({}),
            vec![],
            &settings,
            &json!("org/team-slug"),
            &json!("alice"),
        );
        assert_eq!(result["status"], "fail");
        assert_eq!(
            result["validations"][0]["description"],
            "'alice' is not part of the 'org/team-slug' team'"
        );
    }
}
