use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::github::client::GithubResponse;
use crate::js;
use base64::Engine as _;
use serde_json::{Value, json};

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

fn log_and_rethrow<T>(error: JsError, context: &Context, call_name: &str) -> JsResult<T> {
    let log_type = match error.status {
        Some(500) => "github_server_error",
        Some(404) => "http_not_found_error",
        _ => "unknown_github_api_error",
    };
    tracing::error!(
        event_id = %context.event_id,
        repo = %context.repository_full_name(),
        event = %context.event_with_action(),
        call = call_name,
        %error,
        "{log_type}"
    );
    Err(error)
}

fn debug_call(context: &Context, call_name: &str) {
    tracing::debug!(
        event_id = %context.event_id,
        repo = %context.repository_full_name(),
        event = %context.event_with_action(),
        call = call_name,
        "github api call"
    );
}

async fn call(
    context: &Context,
    call_name: &str,
    route: &str,
    params: Value,
) -> JsResult<GithubResponse> {
    debug_call(context, call_name);
    match context.github.request(route, params).await {
        Ok(response) => Ok(response),
        Err(error) => log_and_rethrow(error, context, call_name),
    }
}

async fn call_without_error_log(
    context: &Context,
    call_name: &str,
    route: &str,
    params: Value,
) -> JsResult<GithubResponse> {
    debug_call(context, call_name);
    context.github.request(route, params).await
}

async fn paginate(
    context: &Context,
    call_name: &str,
    route: &str,
    params: Value,
) -> JsResult<Vec<Value>> {
    debug_call(context, call_name);
    match context.github.paginate(route, params).await {
        Ok(items) => Ok(items),
        Err(error) => log_and_rethrow(error, context, call_name),
    }
}

pub async fn list_files(context: &Context, params: Value) -> JsResult<Vec<Value>> {
    paginate(
        context,
        "pulls.listFiles",
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/files",
        params,
    )
    .await
}

pub async fn add_assignees(
    context: &Context,
    issue_number: &Value,
    assignees: &Value,
) -> JsResult<GithubResponse> {
    let params =
        context.repo_params(json!({"issue_number": issue_number, "assignees": assignees}))?;
    call_without_error_log(
        context,
        "issues.addAssignees",
        "POST /repos/{owner}/{repo}/issues/{issue_number}/assignees",
        params,
    )
    .await
}

pub async fn check_user_can_be_assigned(
    context: &Context,
    issue_number: &Value,
    assignee: &Value,
) -> JsResult<Option<Value>> {
    let params =
        context.repo_params(json!({"issue_number": issue_number, "assignee": assignee}))?;
    debug_call(context, "issues.checkUserCanBeAssigned");
    match context
        .github
        .request("GET /repos/{owner}/{repo}/assignees/{assignee}", params)
        .await
    {
        Ok(response) => Ok((response.status == 204).then(|| assignee.clone())),
        Err(error) if error.status == Some(404) => Ok(None),
        Err(error) => log_and_rethrow(error, context, "issues.checkUserCanBeAssigned"),
    }
}

pub async fn create_checks(context: &Context, params: Value) -> JsResult<GithubResponse> {
    let params = context.repo_params(params)?;
    call(
        context,
        "checks.create",
        "POST /repos/{owner}/{repo}/check-runs",
        params,
    )
    .await
}

pub async fn update_checks(context: &Context, params: Value) -> JsResult<GithubResponse> {
    call(
        context,
        "checks.update",
        "PATCH /repos/{owner}/{repo}/check-runs/{check_run_id}",
        params,
    )
    .await
}

pub fn decode_content(data: &Value) -> JsResult<String> {
    let encoded = js::read_string(data, &["content"])?;
    let compact: String = encoded
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let bytes = LENIENT_BASE64.decode(compact).unwrap_or_default();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub async fn get_content(context: &Context, params: Value) -> JsResult<Option<String>> {
    debug_call(context, "repos.getContent");
    match context
        .github
        .request("GET /repos/{owner}/{repo}/contents/{path}", params)
        .await
    {
        Ok(response) => decode_content(&response.data).map(Some),
        Err(error) if error.status == Some(404) => Ok(None),
        Err(error) => log_and_rethrow(error, context, "repos.getContent"),
    }
}

pub async fn list_labels_on_issue(context: &Context, issue_number: &Value) -> JsResult<Vec<Value>> {
    let params = context.repo_params(json!({"issue_number": issue_number}))?;
    let response = call(
        context,
        "issues.listLabelsOnIssue",
        "GET /repos/{owner}/{repo}/issues/{issue_number}/labels",
        params,
    )
    .await?;
    labels_names(&response.data)
}

fn labels_names(data: &Value) -> JsResult<Vec<Value>> {
    match data {
        Value::Array(labels) => labels
            .iter()
            .map(|label| {
                Ok(js::read(Some(label), "name")?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect(),
        _ => Err(JsError::type_error("res.data.map is not a function")),
    }
}

pub async fn add_labels(
    context: &Context,
    issue_number: &Value,
    labels: &Value,
) -> JsResult<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "labels": labels}))?;
    call(
        context,
        "issues.addLabels",
        "POST /repos/{owner}/{repo}/issues/{issue_number}/labels",
        params,
    )
    .await
}

pub async fn set_labels(
    context: &Context,
    issue_number: &Value,
    labels: &Value,
) -> JsResult<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "labels": labels}))?;
    call(
        context,
        "issues.setLabels",
        "PUT /repos/{owner}/{repo}/issues/{issue_number}/labels",
        params,
    )
    .await
}

pub async fn create_comment(
    context: &Context,
    issue_number: &Value,
    body: &Value,
) -> JsResult<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "body": body}))?;
    call(
        context,
        "issues.createComment",
        "POST /repos/{owner}/{repo}/issues/{issue_number}/comments",
        params,
    )
    .await
}

pub async fn list_comments(context: &Context, issue_number: &Value) -> JsResult<Vec<Value>> {
    let params = context.repo_params(json!({"issue_number": issue_number}))?;
    paginate(
        context,
        "issues.listComments",
        "GET /repos/{owner}/{repo}/issues/{issue_number}/comments",
        params,
    )
    .await
}

pub async fn delete_comment(
    context: &Context,
    comment_id: &Value,
) -> JsResult<Option<GithubResponse>> {
    let params = context.repo_params(json!({"comment_id": comment_id}))?;
    debug_call(context, "issues.deleteComment");
    match context
        .github
        .request(
            "DELETE /repos/{owner}/{repo}/issues/comments/{comment_id}",
            params,
        )
        .await
    {
        Ok(response) => Ok(Some(response)),
        Err(error) if error.status == Some(404) => Ok(None),
        Err(error) => log_and_rethrow(error, context, "issues.deleteComment"),
    }
}

pub async fn get_issue(
    context: &Context,
    issue_number: &Value,
) -> JsResult<Option<GithubResponse>> {
    let params = context.repo_params(json!({"issue_number": issue_number}))?;
    debug_call(context, "issues.get");
    match context
        .github
        .request("GET /repos/{owner}/{repo}/issues/{issue_number}", params)
        .await
    {
        Ok(response) => Ok(Some(response)),
        Err(error) if error.status == Some(404) => Ok(None),
        Err(error) => log_and_rethrow(error, context, "issues.get"),
    }
}

pub async fn update_issue_state(
    context: &Context,
    issue_number: &Value,
    state: &str,
) -> JsResult<GithubResponse> {
    let params = context.repo_params(json!({"issue_number": issue_number, "state": state}))?;
    call_without_error_log(
        context,
        "issues.update",
        "PATCH /repos/{owner}/{repo}/issues/{issue_number}",
        params,
    )
    .await
}

pub async fn list_members_in_org(
    context: &Context,
    org: &str,
    team_slug: &str,
) -> JsResult<Vec<Value>> {
    debug_call(context, "teams.listMembersInOrg");
    match context
        .github
        .request(
            "GET /orgs/{org}/teams/{team_slug}/members",
            json!({"org": org, "team_slug": team_slug}),
        )
        .await
    {
        Ok(response) => logins(&response.data),
        Err(error) if error.status == Some(404) => {
            Err(JsError::team_not_found(format!("{org}/{team_slug}")))
        }
        Err(error) => log_and_rethrow(error, context, "teams.listMembersInOrg"),
    }
}

fn logins(data: &Value) -> JsResult<Vec<Value>> {
    match data {
        Value::Array(users) => users
            .iter()
            .map(|user| {
                Ok(js::read(Some(user), "login")?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect(),
        _ => Err(JsError::type_error("res.data.map is not a function")),
    }
}

pub async fn get_membership_for_user_in_org(
    context: &Context,
    org: &str,
    team_slug: &str,
    username: &Value,
) -> JsResult<bool> {
    debug_call(context, "teams.getMembershipForUserInOrg");
    match context
        .github
        .request(
            "GET /orgs/{org}/teams/{team_slug}/memberships/{username}",
            json!({"org": org, "team_slug": team_slug, "username": username}),
        )
        .await
    {
        Ok(response) => Ok(js::as_str(response.data.get("state")) == Some("active")),
        Err(error) if error.status == Some(404) => Ok(false),
        Err(error) => Err(error),
    }
}

pub async fn project_list_columns(context: &Context, params: Value) -> JsResult<Vec<Value>> {
    let response = call(
        context,
        "projects.listColumns",
        "GET /projects/{project_id}/columns",
        params,
    )
    .await?;
    match &response.data {
        Value::Array(columns) => columns
            .iter()
            .map(|column| {
                Ok(js::read(Some(column), "id")?
                    .cloned()
                    .unwrap_or(Value::Null))
            })
            .collect(),
        _ => Err(JsError::type_error("res.data.map is not a function")),
    }
}

pub async fn project_list_for_repo(context: &Context, params: Value) -> JsResult<Vec<Value>> {
    let response = call(
        context,
        "projects.listForRepo",
        "GET /repos/{owner}/{repo}/projects",
        params,
    )
    .await?;
    match &response.data {
        Value::Array(projects) => Ok(projects
            .iter()
            .map(|project| {
                let mut summary = serde_json::Map::new();
                if let Some(id) = project.get("id") {
                    summary.insert("id".to_string(), id.clone());
                }
                if let Some(name) = project.get("name") {
                    summary.insert("name".to_string(), name.clone());
                }
                Value::Object(summary)
            })
            .collect()),
        _ => Err(JsError::type_error("res.data.map is not a function")),
    }
}

pub async fn project_list_cards(context: &Context, params: Value) -> JsResult<GithubResponse> {
    call(
        context,
        "projects.listCards",
        "GET /projects/columns/{column_id}/cards",
        params,
    )
    .await
}

pub async fn search_issues_and_pull_requests(context: &Context, params: Value) -> JsResult<Value> {
    let response = call(
        context,
        "search.issuesAndPullRequests",
        "GET /search/issues",
        params,
    )
    .await?;
    Ok(take_field(response.data, "items"))
}

pub async fn list_collaborators(context: &Context, params: Value) -> JsResult<Vec<Value>> {
    let response = call(
        context,
        "repos.listCollaborators",
        "GET /repos/{owner}/{repo}/collaborators",
        params,
    )
    .await?;
    logins(&response.data)
}

pub async fn get_all_topics(context: &Context, params: Value) -> JsResult<Value> {
    let response = call(
        context,
        "repos.getAllTopics",
        "GET /repos/{owner}/{repo}/topics",
        params,
    )
    .await?;
    Ok(take_field(response.data, "names"))
}

pub async fn compare_commits(context: &Context, params: Value) -> JsResult<Value> {
    let response = call(
        context,
        "repos.compareCommits",
        "GET /repos/{owner}/{repo}/compare/{base}...{head}",
        params,
    )
    .await?;
    Ok(response.data)
}

pub async fn merge_pull_request(
    context: &Context,
    params: Value,
) -> JsResult<Option<GithubResponse>> {
    debug_call(context, "pulls.merge");
    match context
        .github
        .request(
            "PUT /repos/{owner}/{repo}/pulls/{pull_number}/merge",
            params,
        )
        .await
    {
        Ok(response) => Ok(Some(response)),
        Err(error) => {
            if matches!(error.status, Some(405 | 409)) {
                if error
                    .message
                    .to_lowercase()
                    .contains("required status check")
                {
                    return Ok(None);
                }
                tracing::info!(
                    event_id = %context.event_id,
                    repo = %context.repository_full_name(),
                    event = %context.event_with_action(),
                    call = "pulls.merge",
                    %error,
                    "merge_fail_error"
                );
            }
            log_and_rethrow(error, context, "pulls.merge")
        }
    }
}

pub async fn check_if_merged(context: &Context, pull_number: &Value) -> JsResult<bool> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    debug_call(context, "pulls.checkIfMerged");
    match context
        .github
        .request(
            "GET /repos/{owner}/{repo}/pulls/{pull_number}/merge",
            params,
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.status == Some(404) => Ok(false),
        Err(error) => log_and_rethrow(error, context, "pulls.checkIfMerged"),
    }
}

pub async fn get_pull_request(context: &Context, pull_number: &Value) -> JsResult<GithubResponse> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    call_without_error_log(
        context,
        "pulls.get",
        "GET /repos/{owner}/{repo}/pulls/{pull_number}",
        params,
    )
    .await
}

pub async fn request_reviewers(
    context: &Context,
    pull_number: &Value,
    reviewers: Option<&Value>,
    team_reviewers: Option<&Value>,
) -> JsResult<Option<GithubResponse>> {
    let mut extra = serde_json::Map::new();
    extra.insert("pull_number".to_string(), pull_number.clone());
    if let Some(users) = reviewers {
        extra.insert("reviewers".to_string(), users.clone());
    }
    if let Some(teams) = team_reviewers {
        extra.insert("team_reviewers".to_string(), teams.clone());
    }
    let params = context.repo_params(Value::Object(extra))?;
    debug_call(context, "pulls.requestReview");
    match context
        .github
        .request(
            "POST /repos/{owner}/{repo}/pulls/{pull_number}/requested_reviewers",
            params,
        )
        .await
    {
        Ok(response) => Ok(Some(response)),
        Err(error) => {
            tracing::info!(
                event_id = %context.event_id,
                repo = %context.repository_full_name(),
                event = %context.event_with_action(),
                call = "pulls.requestReview",
                %error,
                "request_review_fail_error"
            );
            Ok(None)
        }
    }
}

pub async fn list_pull_requests(context: &Context) -> JsResult<GithubResponse> {
    let mut extra = serde_json::Map::new();
    if let Some(reference) = context.payload.get("ref") {
        extra.insert("base".to_string(), reference.clone());
    }
    let params = context.repo_params(Value::Object(extra))?;
    call(
        context,
        "pulls.list",
        "GET /repos/{owner}/{repo}/pulls",
        params,
    )
    .await
}

pub async fn list_reviews(context: &Context, pull_number: &Value) -> JsResult<Vec<Value>> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    paginate(
        context,
        "pulls.listReviews",
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/reviews",
        params,
    )
    .await
}

pub async fn list_commits(context: &Context, pull_number: &Value) -> JsResult<Vec<Value>> {
    let params = context.repo_params(json!({"pull_number": pull_number}))?;
    let commits = paginate(
        context,
        "pulls.listCommits",
        "GET /repos/{owner}/{repo}/pulls/{pull_number}/commits",
        params,
    )
    .await?;
    commits
        .iter()
        .map(|commit| {
            let details = js::read(Some(commit), "commit")?;
            let author = js::read(details, "author")?;
            Ok(json!({
                "message": js::read(details, "message")?.cloned().unwrap_or(Value::Null),
                "date": js::read(author, "date")?.cloned().unwrap_or(Value::Null),
                "author": author.cloned().unwrap_or(Value::Null),
                "committer": js::read(details, "committer")?.cloned().unwrap_or(Value::Null),
            }))
        })
        .collect()
}

pub async fn rate_limit(
    context_client: &crate::github::client::InstallationClient,
) -> JsResult<GithubResponse> {
    context_client.request("GET /rate_limit", json!({})).await
}
