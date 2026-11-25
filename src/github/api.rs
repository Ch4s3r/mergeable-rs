use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::client::GithubResponse;
use crate::json_value;
use base64::Engine as _;
use serde_json::{Value, json};

const NOT_FOUND: u16 = 404;
const NO_CONTENT: u16 = 204;

const LENIENT_BASE64: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

fn take_field(data: Value, key: &str) -> Value {
    match data {
        Value::Object(mut fields) => fields.shift_remove(key).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn failure_log_type(error: &Error) -> &'static str {
    match error.status() {
        Some(500) => "github_server_error",
        Some(404) => "http_not_found_error",
        _ => "unknown_github_api_error",
    }
}

fn log_failure(error: Error, route: &str) -> Error {
    tracing::error!(
        route,
        log_type = failure_log_type(&error),
        %error,
        "github.call.failed"
    );
    error
}

fn log_call(route: &str) {
    tracing::debug!(route, "github.call");
}

async fn call(context: &Context, route: &str, params: Value) -> Result<GithubResponse> {
    log_call(route);
    context
        .github
        .request(route, params)
        .await
        .map_err(|error| log_failure(error, route))
}

async fn call_without_error_log(
    context: &Context,
    route: &str,
    params: Value,
) -> Result<GithubResponse> {
    log_call(route);
    context.github.request(route, params).await
}

async fn paginate(context: &Context, route: &str, params: Value) -> Result<Vec<Value>> {
    log_call(route);
    context
        .github
        .paginate(route, params)
        .await
        .map_err(|error| log_failure(error, route))
}

async fn call_treating_not_found_as_absent(
    context: &Context,
    route: &str,
    params: Value,
) -> Result<Option<GithubResponse>> {
    log_call(route);
    match context.github.request(route, params).await {
        Ok(response) => Ok(Some(response)),
        Err(error) if error.has_status(NOT_FOUND) => Ok(None),
        Err(error) => Err(log_failure(error, route)),
    }
}

pub async fn list_files(context: &Context, params: Value) -> Result<Vec<Value>> {
    paginate(
        context,
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/files",
        params,
    )
    .await
}

pub async fn add_assignees(
    context: &Context,
    issue_number: &Value,
    assignees: &Value,
) -> Result<GithubResponse> {
    let params =
        context.repo_params(json!({"issue_number": issue_number, "assignees": assignees}))?;
    call_without_error_log(
        context,
        "POST /repos/{owner}/{repo}/issues/{issue_number}/assignees",
        params,
    )
    .await
}

pub async fn check_user_can_be_assigned(
    context: &Context,
    issue_number: &Value,
    assignee: &Value,
) -> Result<Option<Value>> {
    let params =
        context.repo_params(json!({"issue_number": issue_number, "assignee": assignee}))?;
    let response = call_treating_not_found_as_absent(
        context,
        "GET /repos/{owner}/{repo}/assignees/{assignee}",
        params,
    )
    .await?;
    Ok(response
        .filter(|response| response.status == NO_CONTENT)
        .map(|_| assignee.clone()))
}

pub async fn create_checks(context: &Context, params: Value) -> Result<GithubResponse> {
    let params = context.repo_params(params)?;
    call(context, "POST /repos/{owner}/{repo}/check-runs", params).await
}

pub async fn update_checks(context: &Context, params: Value) -> Result<GithubResponse> {
    call(
        context,
        "PATCH /repos/{owner}/{repo}/check-runs/{check_run_id}",
        params,
    )
    .await
}

pub fn decode_content(data: &Value) -> Result<String> {
    let encoded = json_value::read_string(data, &["content"])?;
    let compact: String = encoded
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let bytes = LENIENT_BASE64.decode(compact).unwrap_or_default();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn fetch_content(context: &Context, params: Value) -> Result<Option<String>> {
    call_treating_not_found_as_absent(context, "GET /repos/{owner}/{repo}/contents/{path}", params)
        .await?
        .map(|response| decode_content(&response.data))
        .transpose()
}

pub async fn list_labels_on_issue(context: &Context, issue_number: &Value) -> Result<Vec<Value>> {
    let params = context.repo_params(json!({"issue_number": issue_number}))?;
    let response = call(
        context,
        "GET /repos/{owner}/{repo}/issues/{issue_number}/labels",
        params,
    )
    .await?;
    take_from_each(response.data, "name")
}

fn take_from_each(data: Value, key: &str) -> Result<Vec<Value>> {
    match data {
        Value::Array(items) => items
            .into_iter()
            .map(|mut item| Ok(json_value::take(Some(&mut item), key)?.unwrap_or(Value::Null)))
            .collect(),
        _ => Err(Error::type_mismatch("res.data.map is not a function")),
    }
}

pub async fn add_labels(
    context: &Context,
    issue_number: &Value,
    labels: &Value,
) -> Result<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "labels": labels}))?;
    call(
        context,
        "POST /repos/{owner}/{repo}/issues/{issue_number}/labels",
        params,
    )
    .await
}

pub async fn set_labels(
    context: &Context,
    issue_number: &Value,
    labels: &Value,
) -> Result<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "labels": labels}))?;
    call(
        context,
        "PUT /repos/{owner}/{repo}/issues/{issue_number}/labels",
        params,
    )
    .await
}

pub async fn create_comment(
    context: &Context,
    issue_number: &Value,
    body: Value,
) -> Result<GithubResponse> {
    let mut extra = serde_json::Map::new();
    extra.insert("issue_number".to_string(), issue_number.clone());
    extra.insert("body".to_string(), body);
    let params = context.repo_params(Value::Object(extra))?;
    call(
        context,
        "POST /repos/{owner}/{repo}/issues/{issue_number}/comments",
        params,
    )
    .await
}

pub async fn list_comments(context: &Context, issue_number: &Value) -> Result<Vec<Value>> {
    let params = context.repo_params(json!({"issue_number": issue_number}))?;
    paginate(
        context,
        "GET /repos/{owner}/{repo}/issues/{issue_number}/comments",
        params,
    )
    .await
}

pub async fn delete_comment(
    context: &Context,
    comment_id: &Value,
) -> Result<Option<GithubResponse>> {
    let params = context.repo_params(json!({"comment_id": comment_id}))?;
    call_treating_not_found_as_absent(
        context,
        "DELETE /repos/{owner}/{repo}/issues/comments/{comment_id}",
        params,
    )
    .await
}

pub async fn fetch_issue(
    context: &Context,
    issue_number: &Value,
) -> Result<Option<GithubResponse>> {
    let params = context.repo_params(json!({"issue_number": issue_number}))?;
    call_treating_not_found_as_absent(
        context,
        "GET /repos/{owner}/{repo}/issues/{issue_number}",
        params,
    )
    .await
}

pub async fn update_issue_state(
    context: &Context,
    issue_number: &Value,
    state: &str,
) -> Result<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "state": state}))?;
    call_without_error_log(
        context,
        "PATCH /repos/{owner}/{repo}/issues/{issue_number}",
        params,
    )
    .await
}

pub async fn list_members_in_org(
    context: &Context,
    org: &str,
    team_slug: &str,
) -> Result<Vec<Value>> {
    let response = call_treating_not_found_as_absent(
        context,
        "GET /orgs/{org}/teams/{team_slug}/members",
        json!({"org": org, "team_slug": team_slug}),
    )
    .await?
    .ok_or_else(|| Error::team_not_found(format!("{org}/{team_slug}")))?;
    take_from_each(response.data, "login")
}

pub async fn fetch_membership_for_user_in_org(
    context: &Context,
    org: &str,
    team_slug: &str,
    username: &Value,
) -> Result<bool> {
    let route = "GET /orgs/{org}/teams/{team_slug}/memberships/{username}";
    log_call(route);
    match context
        .github
        .request(
            route,
            json!({"org": org, "team_slug": team_slug, "username": username}),
        )
        .await
    {
        Ok(response) => Ok(json_value::as_str(response.data.get("state")) == Some("active")),
        Err(error) if error.has_status(NOT_FOUND) => Ok(false),
        Err(error) => Err(error),
    }
}

pub async fn project_list_columns(context: &Context, params: Value) -> Result<Vec<Value>> {
    let response = call(context, "GET /projects/{project_id}/columns", params).await?;
    take_from_each(response.data, "id")
}

pub async fn project_list_for_repo(context: &Context, params: Value) -> Result<Vec<Value>> {
    let response = call(context, "GET /repos/{owner}/{repo}/projects", params).await?;
    match response.data {
        Value::Array(projects) => Ok(projects
            .into_iter()
            .map(|mut project| {
                let mut summary = serde_json::Map::new();
                if let Some(id) = project.get_mut("id") {
                    summary.insert("id".to_string(), id.take());
                }
                if let Some(name) = project.get_mut("name") {
                    summary.insert("name".to_string(), name.take());
                }
                Value::Object(summary)
            })
            .collect()),
        _ => Err(Error::type_mismatch("res.data.map is not a function")),
    }
}

pub async fn project_list_cards(context: &Context, params: Value) -> Result<GithubResponse> {
    call(context, "GET /projects/columns/{column_id}/cards", params).await
}

pub async fn search_issues_and_pull_requests(context: &Context, params: Value) -> Result<Value> {
    let response = call(context, "GET /search/issues", params).await?;
    Ok(take_field(response.data, "items"))
}

pub async fn list_collaborators(context: &Context, params: Value) -> Result<Vec<Value>> {
    let response = call(context, "GET /repos/{owner}/{repo}/collaborators", params).await?;
    take_from_each(response.data, "login")
}

pub async fn fetch_all_topics(context: &Context, params: Value) -> Result<Value> {
    let response = call(context, "GET /repos/{owner}/{repo}/topics", params).await?;
    Ok(take_field(response.data, "names"))
}

pub async fn compare_commits(context: &Context, params: Value) -> Result<Value> {
    let response = call(
        context,
        "GET /repos/{owner}/{repo}/compare/{base}...{head}",
        params,
    )
    .await?;
    Ok(response.data)
}

pub async fn merge_pull_request(
    context: &Context,
    params: Value,
) -> Result<Option<GithubResponse>> {
    let route = "PUT /repos/{owner}/{repo}/pulls/{pull_number}/merge";
    log_call(route);
    match context.github.request(route, params).await {
        Ok(response) => Ok(Some(response)),
        Err(error) => {
            if matches!(error.status(), Some(405 | 409)) {
                if error
                    .message()
                    .to_lowercase()
                    .contains("required status check")
                {
                    return Ok(None);
                }
                tracing::info!(route, %error, "github.merge.refused");
            }
            Err(log_failure(error, route))
        }
    }
}

pub async fn check_if_merged(context: &Context, pull_number: &Value) -> Result<bool> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    let merge = call_treating_not_found_as_absent(
        context,
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/merge",
        params,
    )
    .await?;
    Ok(merge.is_some())
}

pub async fn fetch_pull_request(context: &Context, pull_number: &Value) -> Result<GithubResponse> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    call_without_error_log(
        context,
        "GET /repos/{owner}/{repo}/pulls/{pull_number}",
        params,
    )
    .await
}

pub async fn request_reviewers(
    context: &Context,
    pull_number: &Value,
    reviewers: Option<Value>,
    team_reviewers: Option<Value>,
) -> Result<Option<GithubResponse>> {
    let mut extra = serde_json::Map::new();
    extra.insert("pull_number".to_string(), pull_number.clone());
    if let Some(users) = reviewers {
        extra.insert("reviewers".to_string(), users);
    }
    if let Some(teams) = team_reviewers {
        extra.insert("team_reviewers".to_string(), teams);
    }
    let params = context.repo_params(Value::Object(extra))?;
    let route = "POST /repos/{owner}/{repo}/pulls/{pull_number}/requested_reviewers";
    log_call(route);
    match context.github.request(route, params).await {
        Ok(response) => Ok(Some(response)),
        Err(error) => {
            tracing::info!(route, %error, "github.review_request.failed");
            Ok(None)
        }
    }
}

pub async fn list_pull_requests(context: &Context) -> Result<GithubResponse> {
    let mut extra = serde_json::Map::new();
    if let Some(reference) = context.payload.get("ref") {
        extra.insert("base".to_string(), reference.clone());
    }
    let params = context.repo_params(Value::Object(extra))?;
    call(context, "GET /repos/{owner}/{repo}/pulls", params).await
}

pub async fn list_reviews(context: &Context, pull_number: &Value) -> Result<Vec<Value>> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    paginate(
        context,
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/reviews",
        params,
    )
    .await
}

pub async fn list_commits(context: &Context, pull_number: &Value) -> Result<Vec<Value>> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    let commits = paginate(
        context,
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/commits",
        params,
    )
    .await?;
    commits.into_iter().map(commit_summary).collect()
}

fn commit_summary(mut commit: Value) -> Result<Value> {
    let mut details = json_value::take(Some(&mut commit), "commit")?;
    let author = json_value::read(details.as_ref(), "author")?;
    let date = json_value::read(author, "date")?
        .cloned()
        .unwrap_or(Value::Null);
    let message = json_value::take(details.as_mut(), "message")?.unwrap_or(Value::Null);
    let author = json_value::take(details.as_mut(), "author")?.unwrap_or(Value::Null);
    let committer = json_value::take(details.as_mut(), "committer")?.unwrap_or(Value::Null);
    let mut summary = serde_json::Map::new();
    summary.insert("message".to_string(), message);
    summary.insert("date".to_string(), date);
    summary.insert("author".to_string(), author);
    summary.insert("committer".to_string(), committer);
    Ok(Value::Object(summary))
}
