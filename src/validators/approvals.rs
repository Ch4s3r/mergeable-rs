use crate::app_config::JiraSettings;
use crate::context::Context;
use crate::error::{Error, ErrorKind, Result};
use crate::event_aware;
use crate::github::api;
use crate::json_value;
use crate::registry::Registry;
use crate::results::{self, Status};
use crate::validators::options_processor::{assignees, owners, requested_reviewers, teams};
use crate::validators::{self, DEFAULT_SUPPORTED_OPTIONS, Validator};
use crate::value_lists;
use async_trait::async_trait;
use serde_json::{Map, Value, json};

const NO_CHANGES_REQUESTED_MESSAGE: &str = "No Changes are Requested";
const DEFAULT_BLOCK_MESSAGE: &str = "Please resolve all the changes requested";

pub struct Approvals;

pub struct RequiredReviewerList {
    pub required_reviewers: Vec<Value>,
    pub owner_list: Vec<Value>,
    pub assignee_list: Vec<Value>,
    pub requested_reviewer_list: Vec<Value>,
}

fn validator_context() -> Value {
    json!({"name": "approvals"})
}

fn lower_case_state(review: &Value) -> Result<String> {
    match json_value::read(Some(review), "state")? {
        Some(Value::String(state)) => Ok(state.to_lowercase()),
        None => Err(Error::missing_property("toLowerCase")),
        Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'toLowerCase')",
        )),
        Some(_) => Err(Error::type_mismatch(
            "element.state.toLowerCase is not a function",
        )),
    }
}

fn review_decision_fields(mut review: Value) -> Value {
    let decision_fields = ["state", "submitted_at", "user"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), review.get_mut(key)?.take())))
        .collect();
    Value::Object(decision_fields)
}

pub fn latest_review_per_user(reviews: Vec<Value>) -> Result<Vec<Value>> {
    let mut relevant_reviews = Vec::new();
    for review in reviews {
        if lower_case_state(&review)? != "commented" {
            relevant_reviews.push(review_decision_fields(review));
        }
    }
    let ordered = value_lists::order_by_path(
        relevant_reviews,
        &["submitted_at"],
        value_lists::SortOrder::Descending,
    );
    Ok(value_lists::unique_by_path(ordered, &["user", "login"]))
}

pub fn find_reviewers_by_state(latest_reviews: &[Value], state: &str) -> Result<Vec<Value>> {
    let mut reviewers = Vec::new();
    for review in latest_reviews {
        if lower_case_state(review)? == state {
            let reviewer = match review.get("user") {
                Some(user) if json_value::is_truthy(user) => {
                    user.get("login").cloned().unwrap_or(Value::Null)
                }
                Some(falsy_user) => falsy_user.clone(),
                None => Value::Null,
            };
            reviewers.push(reviewer);
        }
    }
    Ok(reviewers)
}

pub fn process_block_option(block_option: &Value, latest_reviews: &[Value]) -> Result<Value> {
    let description = match block_option
        .get("message")
        .filter(|message| json_value::is_truthy(message))
    {
        Some(message) => message.clone(),
        None => Value::String(DEFAULT_BLOCK_MESSAGE.to_string()),
    };
    let changes_requested_reviewers = find_reviewers_by_state(latest_reviews, "changes_requested")?;
    let is_mergeable = changes_requested_reviewers.is_empty();
    Ok(results::construct_output(
        &validator_context(),
        Value::Array(changes_requested_reviewers),
        block_option,
        Status::from_check(is_mergeable),
        if is_mergeable {
            Value::String(NO_CHANGES_REQUESTED_MESSAGE.to_string())
        } else {
            description
        },
        None,
    ))
}

fn take_truthy_option(settings: &mut Value, key: &str) -> Option<Value> {
    let Value::Object(map) = settings else {
        return None;
    };
    if !map.get(key).is_some_and(json_value::is_truthy) {
        return None;
    }
    map.shift_remove(key)
}

fn required_option(settings: &Value, key: &str) -> bool {
    settings
        .get("required")
        .filter(|value| json_value::is_truthy(value))
        .is_some_and(|required| json_value::truthy(json_value::get(required, key)))
}

fn append_unique(mut required_reviewers: Vec<Value>, additions: &[Value]) -> Vec<Value> {
    if additions.is_empty() {
        return required_reviewers;
    }
    required_reviewers.extend_from_slice(additions);
    value_lists::unique_owned(required_reviewers)
}

#[must_use]
pub fn remove_pull_request_creator(
    mut required_reviewers: Vec<Value>,
    creator: &Value,
) -> Vec<Value> {
    if let Some(position) = required_reviewers
        .iter()
        .position(|reviewer| json_value::strict_equals(reviewer, creator))
    {
        required_reviewers.remove(position);
    }
    required_reviewers
}

async fn required_reviewer_list(
    context: &Context,
    settings: &Value,
) -> Result<RequiredReviewerList> {
    let payload = event_aware::payload(context);
    let creator =
        json_value::read(json_value::read(payload, "user")?, "login")?.unwrap_or(&Value::Null);
    let mut required_reviewers = settings
        .get("required")
        .filter(|value| json_value::is_truthy(value))
        .and_then(|required| json_value::get(required, "reviewers"))
        .filter(|value| json_value::is_truthy(value))
        .map(|reviewers| json_value::as_array_or_wrap(reviewers).to_vec())
        .unwrap_or_default();

    let mut owner_list = Vec::new();
    if required_option(settings, "owners") {
        owner_list = match owners::process(payload.unwrap_or(&Value::Null), context).await {
            Ok(list) => list,
            Err(error) if error.kind() == ErrorKind::TeamNotFound => {
                return Err(Error::missing_property("length"));
            }
            Err(error) => return Err(error),
        };
    }
    required_reviewers = append_unique(required_reviewers, &owner_list);

    let assignee_list = if required_option(settings, "assignees") {
        assignees::process(payload)?
    } else {
        Vec::new()
    };
    required_reviewers = append_unique(required_reviewers, &assignee_list);

    let requested_reviewer_list = if required_option(settings, "requested_reviewers") {
        requested_reviewers::process(payload)?
    } else {
        Vec::new()
    };
    required_reviewers = append_unique(required_reviewers, &requested_reviewer_list);

    Ok(RequiredReviewerList {
        required_reviewers: remove_pull_request_creator(required_reviewers, creator),
        owner_list,
        assignee_list,
        requested_reviewer_list,
    })
}

#[must_use]
pub fn with_resolved_required_option(mut settings: Value, list: RequiredReviewerList) -> Value {
    if list.required_reviewers.is_empty() {
        return settings;
    }
    if let Value::Object(map) = &mut settings {
        let mut required = Map::new();
        required.insert("owners".to_string(), Value::Array(list.owner_list));
        required.insert("assignees".to_string(), Value::Array(list.assignee_list));
        required.insert(
            "reviewers".to_string(),
            Value::Array(list.required_reviewers),
        );
        required.insert(
            "requested_reviewers".to_string(),
            Value::Array(list.requested_reviewer_list),
        );
        map.insert("required".to_string(), Value::Object(required));
    }
    settings
}

#[must_use]
pub fn exclude_users(
    mut approved_reviewers: Vec<Value>,
    exclude_option: Option<&Value>,
) -> Vec<Value> {
    if let Some(users) = exclude_option
        .and_then(|exclude| json_value::get(exclude, "users"))
        .filter(|users| json_value::is_truthy(users))
    {
        let excluded_users = json_value::as_array_or_wrap(users);
        approved_reviewers.retain(|reviewer| !value_lists::contains(excluded_users, reviewer));
    }
    approved_reviewers
}

async fn limit_to_allowed_sources(
    context: &Context,
    limit_option: &Value,
    approved_reviewers: Vec<Value>,
) -> Result<(Vec<Value>, Value)> {
    let payload = event_aware::payload(context);
    let mut owner_members = Vec::new();
    let team_names = json_value::get(limit_option, "teams")
        .filter(|value| json_value::is_truthy(value))
        .map_or(&[][..], json_value::as_array_or_wrap);
    if json_value::truthy(json_value::get(limit_option, "owners")) {
        owner_members = owners::process(payload.unwrap_or(&Value::Null), context).await?;
    }
    let team_members =
        teams::extract_team_memberships(context, team_names, &approved_reviewers).await?;
    let mut allowed_members = value_lists::union(&team_members, &owner_members);
    if let Some(limit_users) =
        json_value::get(limit_option, "users").filter(|value| json_value::is_truthy(value))
    {
        allowed_members =
            value_lists::union(allowed_members, value_lists::array_items(Some(limit_users)));
    }
    let counted_reviewers: Vec<Value> =
        value_lists::intersection(&approved_reviewers, &allowed_members)
            .into_iter()
            .cloned()
            .collect();
    let info = results::construct_info_output(
        &validator_context(),
        Value::Array(counted_reviewers.clone()),
        limit_option,
        Value::String("Only approvals from following sources are counted".to_string()),
    );
    Ok((counted_reviewers, info))
}

async fn process_options_raw(
    settings: &Value,
    approved_reviewers: Vec<Value>,
    jira: &JiraSettings,
) -> Result<Vec<Value>> {
    let options_context = validators::options_context(settings, &DEFAULT_SUPPORTED_OPTIONS);
    let raw_output = validators::options::process(
        &options_context,
        &Value::Array(approved_reviewers),
        settings,
        true,
        jira,
    )
    .await?;
    Ok(json_value::into_array_or_wrap(raw_output))
}

#[async_trait]
impl Validator for Approvals {
    fn name(&self) -> &'static str {
        "approvals"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Value {
        json!({
            "min": {"count": "number", "message": "string"},
            "max": {"count": "number", "message": "string"},
            "required": {
                "reviewers": "array",
                "owners": "boolean",
                "assignees": "boolean",
                "requested_reviewers": "boolean",
                "message": "string",
            },
            "block": {"changes_requested": "boolean", "message": "string"},
            "limit": {"teams": "array", "users": "array", "owners": "boolean"},
            "exclude": {"users": "array"},
        })
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> Result<Value> {
        let mut remaining_settings = settings.clone();
        let block_option = take_truthy_option(&mut remaining_settings, "block");
        let limit_option = take_truthy_option(&mut remaining_settings, "limit");
        let exclude_option = take_truthy_option(&mut remaining_settings, "exclude");

        let pull_number =
            json_value::read(event_aware::payload(context), "number")?.unwrap_or(&Value::Null);
        let reviews = api::list_reviews(context, pull_number).await?;

        let required_list = required_reviewer_list(context, &remaining_settings).await?;
        let options_settings = with_resolved_required_option(remaining_settings, required_list);

        let latest_reviews = latest_review_per_user(reviews)?;
        let mut approved_reviewers = find_reviewers_by_state(&latest_reviews, "approved")?;
        let mut output = Vec::new();
        if let Some(limit) = &limit_option {
            let (counted_reviewers, info) =
                limit_to_allowed_sources(context, limit, approved_reviewers).await?;
            approved_reviewers = counted_reviewers;
            output.push(info);
        }
        approved_reviewers = exclude_users(approved_reviewers, exclude_option.as_ref());

        output.extend(
            process_options_raw(
                &options_settings,
                approved_reviewers,
                &context.app.behavior.jira,
            )
            .await?,
        );

        if let Some(block) = block_option
            .as_ref()
            .filter(|block| json_value::truthy(json_value::get(block, "changes_requested")))
        {
            output.push(process_block_option(block, &latest_reviews)?);
        }

        Ok(results::consolidate_result(output, &validator_context()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(login: &str, state: &str, submitted_at: &str) -> Value {
        json!({"user": {"login": login}, "state": state, "submitted_at": submitted_at, "body": "review text"})
    }

    fn reviewers_with_state(reviews: &[Value], state: &str) -> Result<Vec<Value>> {
        find_reviewers_by_state(&latest_review_per_user(reviews.to_vec())?, state)
    }

    #[test]
    fn latest_review_per_user_decides_the_approval() {
        let reviews = vec![
            review("bob", "APPROVED", "2024-01-01T10:00:00Z"),
            review("bob", "CHANGES_REQUESTED", "2024-01-02T10:00:00Z"),
            review("carol", "APPROVED", "2024-01-01T10:00:00Z"),
        ];
        assert_eq!(
            reviewers_with_state(&reviews, "approved").unwrap(),
            vec![json!("carol")]
        );
        assert_eq!(
            reviewers_with_state(&reviews, "changes_requested").unwrap(),
            vec![json!("bob")]
        );
    }

    #[test]
    fn later_comment_does_not_nullify_an_approval() {
        let reviews = vec![
            review("bob", "APPROVED", "2024-01-01T10:00:00Z"),
            review("bob", "COMMENTED", "2024-01-02T10:00:00Z"),
        ];
        assert_eq!(
            reviewers_with_state(&reviews, "approved").unwrap(),
            vec![json!("bob")]
        );
    }

    #[test]
    fn later_pending_review_nullifies_an_approval() {
        let reviews = vec![
            review("bob", "APPROVED", "2024-01-01T10:00:00Z"),
            review("bob", "PENDING", "2024-01-02T10:00:00Z"),
        ];
        assert_eq!(
            reviewers_with_state(&reviews, "approved").unwrap(),
            [] as [serde_json::Value; 0]
        );
    }

    #[test]
    fn requested_changes_block_with_custom_message() {
        let reviews = vec![review("bob", "CHANGES_REQUESTED", "2024-01-01T10:00:00Z")];
        let output = process_block_option(
            &json!({"changes_requested": true, "message": "Fix it"}),
            &latest_review_per_user(reviews).unwrap(),
        )
        .unwrap();
        assert_eq!(output["status"], "fail");
        assert_eq!(output["description"], "Fix it");
        assert_eq!(output["details"]["input"], json!(["bob"]));
    }

    #[test]
    fn no_requested_changes_passes_the_block_option() {
        let reviews = vec![review("bob", "APPROVED", "2024-01-01T10:00:00Z")];
        let output = process_block_option(
            &json!({"changes_requested": true}),
            &latest_review_per_user(reviews).unwrap(),
        )
        .unwrap();
        assert_eq!(output["status"], "pass");
        assert_eq!(output["description"], NO_CHANGES_REQUESTED_MESSAGE);
    }

    #[test]
    fn pull_request_creator_is_removed_from_required_reviewers() {
        let remaining =
            remove_pull_request_creator(vec![json!("alice"), json!("bob")], &json!("alice"));
        assert_eq!(remaining, vec![json!("bob")]);
    }

    #[test]
    fn resolved_required_option_replaces_message_and_keeps_position() {
        let settings = json!({"do": "approvals", "required": {"reviewers": ["bob"], "message": "custom"}, "min": {"count": 1}});
        let list = RequiredReviewerList {
            required_reviewers: vec![json!("bob"), json!("carol")],
            owner_list: vec![],
            assignee_list: vec![json!("carol")],
            requested_reviewer_list: vec![],
        };
        let resolved = with_resolved_required_option(settings, list);
        assert_eq!(
            json_value::stringify(&resolved),
            r#"{"do":"approvals","required":{"owners":[],"assignees":["carol"],"reviewers":["bob","carol"],"requested_reviewers":[]},"min":{"count":1}}"#
        );
    }

    #[test]
    fn excluded_users_do_not_count_as_approvals() {
        let approved = exclude_users(
            vec![json!("bob"), json!("carol")],
            Some(&json!({"users": ["bob"]})),
        );
        assert_eq!(approved, vec![json!("carol")]);
    }

    #[test]
    fn review_without_state_fails_with_a_missing_property_error() {
        let error =
            reviewers_with_state(&[json!({"user": {"login": "bob"}})], "approved").unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'toLowerCase')"
        );
    }
}
