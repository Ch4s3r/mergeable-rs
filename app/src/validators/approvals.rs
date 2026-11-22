use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::lodash;
use crate::registry::Registry;
use crate::results;
use crate::validators::options_processor::{assignees, owners, requested_reviewers, teams};
use crate::validators::{self, DEFAULT_SUPPORTED_OPTIONS, Validator};
use async_trait::async_trait;
use serde_json::{Value, json};

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

fn lower_case_state(review: &Value) -> JsResult<String> {
    match js::read(Some(review), "state")? {
        Some(Value::String(state)) => Ok(state.to_lowercase()),
        None => Err(JsError::reading_property_of_undefined("toLowerCase")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'toLowerCase')",
        )),
        Some(_) => Err(JsError::type_error(
            "element.state.toLowerCase is not a function",
        )),
    }
}

fn review_decision_fields(review: &Value) -> Value {
    let decision_fields = ["state", "submitted_at", "user"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), review.get(key)?.clone())))
        .collect();
    Value::Object(decision_fields)
}

pub fn latest_review_per_user(reviews: &[Value]) -> JsResult<Vec<Value>> {
    let mut relevant_reviews = Vec::new();
    for review in reviews {
        if lower_case_state(review)? != "commented" {
            relevant_reviews.push(review_decision_fields(review));
        }
    }
    let ordered = lodash::order_by_path(relevant_reviews, &["submitted_at"], true);
    Ok(lodash::uniq_by_path(ordered, &["user", "login"]))
}

pub fn find_reviewers_by_state(latest_reviews: &[Value], state: &str) -> JsResult<Vec<Value>> {
    let mut reviewers = Vec::new();
    for review in latest_reviews {
        if lower_case_state(review)? == state {
            let reviewer = match review.get("user") {
                Some(user) if js::is_truthy(user) => {
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

pub fn process_block_option(block_option: &Value, latest_reviews: &[Value]) -> JsResult<Value> {
    let description = match block_option
        .get("message")
        .filter(|message| js::is_truthy(message))
    {
        Some(message) => message.clone(),
        None => Value::String(DEFAULT_BLOCK_MESSAGE.to_string()),
    };
    let changes_requested_reviewers = find_reviewers_by_state(latest_reviews, "changes_requested")?;
    let is_mergeable = changes_requested_reviewers.is_empty();
    Ok(results::construct_output(
        &validator_context(),
        &Value::Array(changes_requested_reviewers),
        block_option,
        if is_mergeable { "pass" } else { "fail" },
        if is_mergeable {
            Value::String(NO_CHANGES_REQUESTED_MESSAGE.to_string())
        } else {
            description
        },
        None,
    ))
}

fn take_truthy_option(settings: &mut Value, key: &str) -> Option<Value> {
    let option = settings
        .get(key)
        .filter(|value| js::is_truthy(value))
        .cloned()?;
    if let Value::Object(map) = settings {
        map.shift_remove(key);
    }
    Some(option)
}

fn required_option(settings: &Value, key: &str) -> bool {
    settings
        .get("required")
        .filter(|value| js::is_truthy(value))
        .is_some_and(|required| js::truthy(js::get(required, key)))
}

fn append_unique(mut required_reviewers: Vec<Value>, additions: &[Value]) -> Vec<Value> {
    if additions.is_empty() {
        return required_reviewers;
    }
    required_reviewers.extend_from_slice(additions);
    lodash::uniq(&required_reviewers)
}

pub fn remove_pull_request_creator(
    mut required_reviewers: Vec<Value>,
    creator: &Value,
) -> Vec<Value> {
    if let Some(position) = required_reviewers
        .iter()
        .position(|reviewer| js::strict_equals(reviewer, creator))
    {
        required_reviewers.remove(position);
    }
    required_reviewers
}

async fn required_reviewer_list(
    context: &Context,
    settings: &Value,
) -> JsResult<RequiredReviewerList> {
    let payload = event_aware::get_payload(context);
    let creator = js::read(js::read(payload, "user")?, "login")?
        .cloned()
        .unwrap_or(Value::Null);
    let mut required_reviewers = settings
        .get("required")
        .filter(|value| js::is_truthy(value))
        .and_then(|required| js::get(required, "reviewers"))
        .filter(|value| js::is_truthy(value))
        .map(js::as_array_or_wrap)
        .unwrap_or_default();

    let mut owner_list = Vec::new();
    if required_option(settings, "owners") {
        owner_list = match owners::process(payload.unwrap_or(&Value::Null), context).await {
            Ok(list) => list,
            Err(error) if error.is_named("TeamNotFoundError") => {
                return Err(JsError::reading_property_of_undefined("length"));
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
        required_reviewers: remove_pull_request_creator(required_reviewers, &creator),
        owner_list,
        assignee_list,
        requested_reviewer_list,
    })
}

pub fn with_resolved_required_option(mut settings: Value, list: &RequiredReviewerList) -> Value {
    if list.required_reviewers.is_empty() {
        return settings;
    }
    if let Value::Object(map) = &mut settings {
        map.insert(
            "required".to_string(),
            json!({
                "owners": list.owner_list,
                "assignees": list.assignee_list,
                "reviewers": list.required_reviewers,
                "requested_reviewers": list.requested_reviewer_list,
            }),
        );
    }
    settings
}

pub fn exclude_users(approved_reviewers: Vec<Value>, exclude_option: Option<&Value>) -> Vec<Value> {
    match exclude_option
        .and_then(|exclude| js::get(exclude, "users"))
        .filter(|users| js::is_truthy(users))
    {
        Some(users) => lodash::difference(&approved_reviewers, &js::as_array_or_wrap(users)),
        None => approved_reviewers,
    }
}

async fn limit_to_allowed_sources(
    context: &Context,
    limit_option: &Value,
    approved_reviewers: Vec<Value>,
) -> JsResult<(Vec<Value>, Value)> {
    let payload = event_aware::get_payload(context);
    let mut owner_members = Vec::new();
    let mut team_names = Vec::new();
    if let Some(limit_teams) = js::get(limit_option, "teams").filter(|value| js::is_truthy(value)) {
        team_names = js::concat(&team_names, limit_teams);
    }
    if js::truthy(js::get(limit_option, "owners")) {
        owner_members = owners::process(payload.unwrap_or(&Value::Null), context).await?;
    }
    let mut team_members =
        teams::extract_team_memberships(context, &team_names, &approved_reviewers).await?;
    team_members = lodash::union(&team_members, &owner_members);
    if let Some(limit_users) = js::get(limit_option, "users").filter(|value| js::is_truthy(value)) {
        team_members = lodash::union(&team_members, &lodash::array_like(Some(limit_users)));
    }
    let counted_reviewers = lodash::intersection(&approved_reviewers, &team_members);
    let info = results::construct_output(
        &validator_context(),
        &Value::Array(counted_reviewers.clone()),
        limit_option,
        "info",
        Value::String("Only approvals from following sources are counted".to_string()),
        None,
    );
    Ok((counted_reviewers, info))
}

async fn process_options_raw(
    settings: &Value,
    approved_reviewers: Vec<Value>,
) -> JsResult<Vec<Value>> {
    let supported: Vec<Value> = DEFAULT_SUPPORTED_OPTIONS
        .iter()
        .map(|option| Value::String((*option).to_string()))
        .collect();
    let options_context = json!({
        "name": settings.get("do").cloned().unwrap_or(Value::Null),
        "supportedOptions": supported,
    });
    let raw_output = validators::options::process(
        &options_context,
        &Value::Array(approved_reviewers),
        settings,
        true,
    )
    .await?;
    Ok(js::as_array_or_wrap(&raw_output))
}

#[async_trait]
impl Validator for Approvals {
    fn name(&self) -> &'static str {
        "approvals"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["pull_request.*", "pull_request_review.*"]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
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
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let mut remaining_settings = settings.clone();
        let block_option = take_truthy_option(&mut remaining_settings, "block");
        let limit_option = take_truthy_option(&mut remaining_settings, "limit");
        let exclude_option = take_truthy_option(&mut remaining_settings, "exclude");

        let pull_number = js::read(event_aware::get_payload(context), "number")?
            .cloned()
            .unwrap_or(Value::Null);
        let reviews = api::list_reviews(context, &pull_number).await?;

        let required_list = required_reviewer_list(context, &remaining_settings).await?;
        let options_settings = with_resolved_required_option(remaining_settings, &required_list);

        let latest_reviews = latest_review_per_user(&reviews)?;
        let mut approved_reviewers = find_reviewers_by_state(&latest_reviews, "approved")?;
        let mut output = Vec::new();
        if let Some(limit) = &limit_option {
            let (counted_reviewers, info) =
                limit_to_allowed_sources(context, limit, approved_reviewers).await?;
            approved_reviewers = counted_reviewers;
            output.push(info);
        }
        approved_reviewers = exclude_users(approved_reviewers, exclude_option.as_ref());

        output.extend(process_options_raw(&options_settings, approved_reviewers).await?);

        if let Some(block) = block_option
            .as_ref()
            .filter(|block| js::truthy(js::get(block, "changes_requested")))
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

    fn reviewers_with_state(reviews: &[Value], state: &str) -> JsResult<Vec<Value>> {
        find_reviewers_by_state(&latest_review_per_user(reviews)?, state)
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
        assert!(
            reviewers_with_state(&reviews, "approved")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn requested_changes_block_with_custom_message() {
        let reviews = vec![review("bob", "CHANGES_REQUESTED", "2024-01-01T10:00:00Z")];
        let output = process_block_option(
            &json!({"changes_requested": true, "message": "Fix it"}),
            &latest_review_per_user(&reviews).unwrap(),
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
            &latest_review_per_user(&reviews).unwrap(),
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
        let resolved = with_resolved_required_option(settings, &list);
        assert_eq!(
            js::stringify(&resolved),
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
    fn review_without_state_throws_like_node() {
        let error =
            reviewers_with_state(&[json!({"user": {"login": "bob"}})], "approved").unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'toLowerCase')"
        );
    }
}
