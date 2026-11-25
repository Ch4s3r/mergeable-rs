use crate::app_state::GithubDataCaches;
use crate::cache::NamedCache;
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

fn github_data_key(cache: &NamedCache, id: &str) -> String {
    format!("github-data:{}:{id}", cache.name())
}

async fn through_github_data_cache(
    context: &Context,
    select_cache: impl FnOnce(&GithubDataCaches) -> &NamedCache,
    id: &str,
    fetch: impl Future<Output = Result<Value>>,
) -> Result<Value> {
    match context.app.github_data.as_ref().map(select_cache) {
        Some(cache) => cache.get_or_fetch(&github_data_key(cache, id), fetch).await,
        None => fetch.await,
    }
}

fn into_list(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items,
        Value::Null => Vec::new(),
        single => vec![single],
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
    let members = through_github_data_cache(
        context,
        |caches| &caches.team_members,
        &format!("{org}/{team_slug}"),
        async {
            fetch_members_in_org(context, org, team_slug)
                .await
                .map(Value::Array)
        },
    )
    .await?;
    Ok(into_list(members))
}

async fn fetch_members_in_org(context: &Context, org: &str, team_slug: &str) -> Result<Vec<Value>> {
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
    let is_active_member = through_github_data_cache(
        context,
        |caches| &caches.team_memberships,
        &format!(
            "{org}/{team_slug}/{}",
            json_value::to_display_string(Some(username))
        ),
        async {
            fetch_active_membership(context, org, team_slug, username)
                .await
                .map(Value::Bool)
        },
    )
    .await?;
    Ok(is_active_member == Value::Bool(true))
}

async fn fetch_active_membership(
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

fn repository_cache_id(params: &Value) -> String {
    format!(
        "{}/{}",
        json_value::to_display_string(params.get("owner")),
        json_value::to_display_string(params.get("repo"))
    )
}

pub async fn list_collaborators(context: &Context, params: Value) -> Result<Vec<Value>> {
    let collaborators = through_github_data_cache(
        context,
        |caches| &caches.collaborators,
        &repository_cache_id(&params),
        async move {
            let response = call(context, "GET /repos/{owner}/{repo}/collaborators", params).await?;
            take_from_each(response.data, "login").map(Value::Array)
        },
    )
    .await?;
    Ok(into_list(collaborators))
}

pub async fn fetch_all_topics(context: &Context, params: Value) -> Result<Value> {
    through_github_data_cache(
        context,
        |caches| &caches.repository_topics,
        &repository_cache_id(&params),
        async move {
            let response = call(context, "GET /repos/{owner}/{repo}/topics", params).await?;
            Ok(take_field(response.data, "names"))
        },
    )
    .await
}

pub async fn fetch_organization_default_config(
    context: &Context,
    owner: &str,
    file_path: &str,
) -> Result<Option<String>> {
    let cached_content = through_github_data_cache(
        context,
        |caches| &caches.organization_config,
        &format!("{owner}/{file_path}"),
        async {
            let file_content = fetch_content(
                context,
                json!({"owner": owner, "repo": ".github", "path": file_path}),
            )
            .await?;
            Ok(json!({ "content": file_content }))
        },
    )
    .await?;
    Ok(json_value::as_str(cached_content.get("content")).map(str::to_string))
}

pub async fn forget_organization_default_config(context: &Context, owner: &str, file_path: &str) {
    if let Some(caches) = &context.app.github_data {
        let cache = &caches.organization_config;
        cache
            .delete(&github_data_key(cache, &format!("{owner}/{file_path}")))
            .await;
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::stub_server::{
        StubGithub, StubbedRoute, empty_route, not_found, payment_required, route,
    };

    const PULL_FILES: &str = "/repos/owner/repo/pulls/1/files";
    const SEARCH: &str = "/search/issues";
    const MERGEABLE_CONFIG: &str = "/repos/owner/repo/contents/.github%2Fmergeable.yml";
    const CHECK_RUNS: &str = "/repos/owner/repo/check-runs";
    const CHECK_RUN: &str = "/repos/owner/repo/check-runs/1";
    const ISSUE_LABELS: &str = "/repos/owner/repo/issues/1/labels";
    const ISSUE_ASSIGNEES: &str = "/repos/owner/repo/issues/1/assignees";
    const ASSIGNEE: &str = "/repos/owner/repo/assignees/testAssignee";
    const ISSUE_COMMENTS: &str = "/repos/owner/repo/issues/1/comments";
    const COMMENT: &str = "/repos/owner/repo/issues/comments/1";
    const ISSUE: &str = "/repos/owner/repo/issues/1";
    const TEAM_MEMBERS: &str = "/orgs/org/teams/team/members";
    const PROJECT_COLUMNS: &str = "/projects/1/columns";
    const REPOSITORY_PROJECTS: &str = "/repos/owner/repo/projects";
    const COLUMN_CARDS: &str = "/projects/columns/1/cards";
    const COLLABORATORS: &str = "/repos/owner/repo/collaborators";
    const TOPICS: &str = "/repos/owner/repo/topics";
    const COMPARISON: &str = "/repos/owner/repo/compare/base...head";
    const PULL_MERGE: &str = "/repos/owner/repo/pulls/1/merge";
    const REQUESTED_REVIEWERS: &str = "/repos/owner/repo/pulls/1/requested_reviewers";
    const PULL_REQUEST: &str = "/repos/owner/repo/pulls/12";
    const PULL_REQUESTS: &str = "/repos/owner/repo/pulls";
    const PULL_REVIEWS: &str = "/repos/owner/repo/pulls/1/reviews";
    const PULL_COMMITS: &str = "/repos/owner/repo/pulls/1/commits";

    fn issue_number() -> Value {
        json!(1)
    }

    fn repository() -> Value {
        json!({"owner": "owner", "repo": "repo"})
    }

    fn pull_request_files() -> Value {
        json!({"owner": "owner", "repo": "repo", "pull_number": 1})
    }

    fn assert_payment_required<T: std::fmt::Debug>(outcome: Result<T>) {
        assert_eq!(outcome.unwrap_err().status(), Some(402));
    }

    fn second_page_of(path: &str) -> String {
        format!("{path}?page=2")
    }

    fn two_pages(path: &str, first: Value, second: Value) -> Vec<StubbedRoute> {
        vec![
            route("GET", path, 200, second).only_for_query("page=2"),
            route("GET", path, 200, first).linking_next_page(&second_page_of(path)),
        ]
    }

    #[tokio::test]
    async fn list_files_returns_the_changed_files() {
        let changed_file = |name: &str| json!({"filename": name, "additions": 0, "deletions": 0, "changes": 0, "status": "modified"});
        let github = StubGithub::serving(vec![route(
            "GET",
            PULL_FILES,
            200,
            json!([changed_file("abc.js"), changed_file("def.js")]),
        )])
        .await;
        let files = list_files(&github.pull_request_context(), pull_request_files())
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], changed_file("abc.js"));
    }

    #[tokio::test]
    async fn list_files_collects_every_page() {
        let github = StubGithub::serving(two_pages(
            PULL_FILES,
            json!([{"filename": "abc.js"}]),
            json!([{"filename": "def.js"}]),
        ))
        .await;
        let files = list_files(&github.pull_request_context(), pull_request_files())
            .await
            .unwrap();
        assert_eq!(
            files,
            vec![json!({"filename": "abc.js"}), json!({"filename": "def.js"})]
        );
        assert_eq!(github.requests().len(), 2);
    }

    #[tokio::test]
    async fn list_files_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PULL_FILES)]).await;
        assert_payment_required(
            list_files(&github.pull_request_context(), pull_request_files()).await,
        );
    }

    #[tokio::test]
    async fn search_issues_and_pull_requests_returns_the_found_items() {
        let github = StubGithub::serving(vec![route(
            "GET",
            SEARCH,
            200,
            json!({"total_count": 2, "items": ["issues 1", "PR 2"]}),
        )])
        .await;
        let items = search_issues_and_pull_requests(
            &github.pull_request_context(),
            json!({"q": "repo:owner/repo"}),
        )
        .await
        .unwrap();
        assert_eq!(items, json!(["issues 1", "PR 2"]));
    }

    #[tokio::test]
    async fn search_issues_and_pull_requests_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", SEARCH)]).await;
        assert_payment_required(
            search_issues_and_pull_requests(
                &github.pull_request_context(),
                json!({"q": "repo:owner/repo"}),
            )
            .await,
        );
    }

    fn mergeable_config_file() -> Value {
        json!({"owner": "owner", "repo": "repo", "path": ".github/mergeable.yml"})
    }

    #[tokio::test]
    async fn fetch_content_returns_the_decoded_file_content() {
        let content = "This is the content";
        let github = StubGithub::serving(vec![route(
            "GET",
            MERGEABLE_CONFIG,
            200,
            json!({"content": base64::engine::general_purpose::STANDARD.encode(content)}),
        )])
        .await;
        let fetched = fetch_content(&github.pull_request_context(), mergeable_config_file())
            .await
            .unwrap();
        assert_eq!(fetched.as_deref(), Some(content));
    }

    #[tokio::test]
    async fn fetch_content_returns_none_for_a_missing_file() {
        let github = StubGithub::serving(vec![not_found("GET", MERGEABLE_CONFIG)]).await;
        let fetched = fetch_content(&github.pull_request_context(), mergeable_config_file())
            .await
            .unwrap();
        assert_eq!(fetched, None);
    }

    #[tokio::test]
    async fn fetch_content_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", MERGEABLE_CONFIG)]).await;
        assert_payment_required(
            fetch_content(&github.pull_request_context(), mergeable_config_file()).await,
        );
    }

    #[tokio::test]
    async fn create_checks_returns_the_created_check_run() {
        let github =
            StubGithub::serving(vec![route("POST", CHECK_RUNS, 201, json!({"id": 1}))]).await;
        let response = create_checks(&github.pull_request_context(), json!({"name": "Mergeable"}))
            .await
            .unwrap();
        assert_eq!(response.data["id"], json!(1));
    }

    #[tokio::test]
    async fn create_checks_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("POST", CHECK_RUNS)]).await;
        assert_payment_required(
            create_checks(&github.pull_request_context(), json!({"name": "Mergeable"})).await,
        );
    }

    fn check_run_update() -> Value {
        json!({"owner": "owner", "repo": "repo", "check_run_id": 1, "status": "completed"})
    }

    #[tokio::test]
    async fn update_checks_returns_the_response() {
        let github = StubGithub::serving(vec![route("PATCH", CHECK_RUN, 200, json!({}))]).await;
        let response = update_checks(&github.pull_request_context(), check_run_update())
            .await
            .unwrap();
        assert_eq!(response.data, json!({}));
    }

    #[tokio::test]
    async fn update_checks_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("PATCH", CHECK_RUN)]).await;
        assert_payment_required(
            update_checks(&github.pull_request_context(), check_run_update()).await,
        );
    }

    #[tokio::test]
    async fn list_labels_on_issue_returns_the_label_names() {
        let github = StubGithub::serving(vec![route(
            "GET",
            ISSUE_LABELS,
            200,
            json!([{"name": "one"}, {"name": "two"}]),
        )])
        .await;
        let labels = list_labels_on_issue(&github.pull_request_context(), &issue_number())
            .await
            .unwrap();
        assert_eq!(labels, vec![json!("one"), json!("two")]);
    }

    #[tokio::test]
    async fn list_labels_on_issue_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", ISSUE_LABELS)]).await;
        assert_payment_required(
            list_labels_on_issue(&github.pull_request_context(), &issue_number()).await,
        );
    }

    #[tokio::test]
    async fn add_labels_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "POST",
            ISSUE_LABELS,
            200,
            json!("addLabels call success"),
        )])
        .await;
        let response = add_labels(
            &github.pull_request_context(),
            &issue_number(),
            &json!(["one"]),
        )
        .await
        .unwrap();
        assert_eq!(response.data, json!("addLabels call success"));
    }

    #[tokio::test]
    async fn add_labels_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("POST", ISSUE_LABELS)]).await;
        assert_payment_required(
            add_labels(
                &github.pull_request_context(),
                &issue_number(),
                &json!(["one"]),
            )
            .await,
        );
    }

    #[tokio::test]
    async fn set_labels_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "PUT",
            ISSUE_LABELS,
            200,
            json!("setLabels call success"),
        )])
        .await;
        let response = set_labels(
            &github.pull_request_context(),
            &issue_number(),
            &json!(["one"]),
        )
        .await
        .unwrap();
        assert_eq!(response.data, json!("setLabels call success"));
    }

    #[tokio::test]
    async fn set_labels_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("PUT", ISSUE_LABELS)]).await;
        assert_payment_required(
            set_labels(
                &github.pull_request_context(),
                &issue_number(),
                &json!(["one"]),
            )
            .await,
        );
    }

    #[tokio::test]
    async fn add_assignees_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "POST",
            ISSUE_ASSIGNEES,
            201,
            json!("addAssignees call success"),
        )])
        .await;
        let response = add_assignees(
            &github.pull_request_context(),
            &issue_number(),
            &json!(["testAssignee"]),
        )
        .await
        .unwrap();
        assert_eq!(response.data, json!("addAssignees call success"));
    }

    #[tokio::test]
    async fn add_assignees_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("POST", ISSUE_ASSIGNEES)]).await;
        assert_payment_required(
            add_assignees(
                &github.pull_request_context(),
                &issue_number(),
                &json!(["testAssignee"]),
            )
            .await,
        );
    }

    #[tokio::test]
    async fn check_user_can_be_assigned_returns_the_assignable_user() {
        let github = StubGithub::serving(vec![empty_route("GET", ASSIGNEE, 204)]).await;
        let assignee = check_user_can_be_assigned(
            &github.pull_request_context(),
            &json!("1"),
            &json!("testAssignee"),
        )
        .await
        .unwrap();
        assert_eq!(assignee, Some(json!("testAssignee")));
    }

    #[tokio::test]
    async fn check_user_can_be_assigned_returns_none_when_not_found() {
        let github = StubGithub::serving(vec![not_found("GET", ASSIGNEE)]).await;
        let assignee = check_user_can_be_assigned(
            &github.pull_request_context(),
            &json!("1"),
            &json!("testAssignee"),
        )
        .await
        .unwrap();
        assert_eq!(assignee, None);
    }

    #[tokio::test]
    async fn check_user_can_be_assigned_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", ASSIGNEE)]).await;
        assert_payment_required(
            check_user_can_be_assigned(
                &github.pull_request_context(),
                &json!("1"),
                &json!("testAssignee"),
            )
            .await,
        );
    }

    #[tokio::test]
    async fn create_comment_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "POST",
            ISSUE_COMMENTS,
            201,
            json!("createComment call success"),
        )])
        .await;
        let response = create_comment(&github.pull_request_context(), &issue_number(), json!("hi"))
            .await
            .unwrap();
        assert_eq!(response.data, json!("createComment call success"));
    }

    #[tokio::test]
    async fn create_comment_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("POST", ISSUE_COMMENTS)]).await;
        assert_payment_required(
            create_comment(&github.pull_request_context(), &issue_number(), json!("hi")).await,
        );
    }

    #[tokio::test]
    async fn list_comments_returns_the_comments() {
        let github = StubGithub::serving(vec![route(
            "GET",
            ISSUE_COMMENTS,
            200,
            json!([{"user": {"login": "mergeable[bot]"}}, {"user": {"login": "userA"}}]),
        )])
        .await;
        let comments = list_comments(&github.pull_request_context(), &issue_number())
            .await
            .unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0]["user"]["login"], json!("mergeable[bot]"));
    }

    #[tokio::test]
    async fn list_comments_collects_every_page() {
        let github = StubGithub::serving(two_pages(
            ISSUE_COMMENTS,
            json!([{"id": 1}]),
            json!([{"id": 2}]),
        ))
        .await;
        let comments = list_comments(&github.pull_request_context(), &issue_number())
            .await
            .unwrap();
        assert_eq!(comments, vec![json!({"id": 1}), json!({"id": 2})]);
    }

    #[tokio::test]
    async fn list_comments_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", ISSUE_COMMENTS)]).await;
        assert_payment_required(
            list_comments(&github.pull_request_context(), &issue_number()).await,
        );
    }

    #[tokio::test]
    async fn delete_comment_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "DELETE",
            COMMENT,
            200,
            json!("deleteComment call success"),
        )])
        .await;
        let response = delete_comment(&github.pull_request_context(), &json!(1))
            .await
            .unwrap();
        assert_eq!(
            response.map(|deleted| deleted.data),
            Some(json!("deleteComment call success"))
        );
    }

    #[tokio::test]
    async fn delete_comment_does_not_throw_when_the_comment_is_not_found() {
        let github = StubGithub::serving(vec![not_found("DELETE", COMMENT)]).await;
        let response = delete_comment(&github.pull_request_context(), &json!(1))
            .await
            .unwrap();
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn delete_comment_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("DELETE", COMMENT)]).await;
        assert_payment_required(delete_comment(&github.pull_request_context(), &json!(1)).await);
    }

    #[tokio::test]
    async fn update_issue_state_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "PATCH",
            ISSUE,
            200,
            json!("update Issues call success"),
        )])
        .await;
        let response =
            update_issue_state(&github.pull_request_context(), &issue_number(), "closed")
                .await
                .unwrap();
        assert_eq!(response.data, json!("update Issues call success"));
        assert_eq!(github.requests()[0].body, json!({"state": "closed"}));
    }

    #[tokio::test]
    async fn update_issue_state_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("PATCH", ISSUE)]).await;
        assert_payment_required(
            update_issue_state(&github.pull_request_context(), &issue_number(), "closed").await,
        );
    }

    #[tokio::test]
    async fn fetch_issue_returns_the_issue() {
        let github =
            StubGithub::serving(vec![route("GET", ISSUE, 200, json!("get Issue success"))]).await;
        let issue = fetch_issue(&github.pull_request_context(), &issue_number())
            .await
            .unwrap();
        assert_eq!(
            issue.map(|found| found.data),
            Some(json!("get Issue success"))
        );
    }

    #[tokio::test]
    async fn fetch_issue_returns_none_when_not_found() {
        let github = StubGithub::serving(vec![not_found("GET", ISSUE)]).await;
        let issue = fetch_issue(&github.pull_request_context(), &issue_number())
            .await
            .unwrap();
        assert!(issue.is_none());
    }

    #[tokio::test]
    async fn fetch_issue_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", ISSUE)]).await;
        assert_payment_required(fetch_issue(&github.pull_request_context(), &issue_number()).await);
    }

    #[tokio::test]
    async fn list_members_in_org_returns_the_member_logins() {
        let github = StubGithub::serving(vec![route(
            "GET",
            TEAM_MEMBERS,
            200,
            json!([{"login": "member1"}, {"login": "member2"}]),
        )])
        .await;
        let members = list_members_in_org(&github.pull_request_context(), "org", "team")
            .await
            .unwrap();
        assert_eq!(members, vec![json!("member1"), json!("member2")]);
    }

    #[tokio::test]
    async fn list_members_in_org_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", TEAM_MEMBERS)]).await;
        assert_payment_required(
            list_members_in_org(&github.pull_request_context(), "org", "team").await,
        );
    }

    #[tokio::test]
    async fn fetch_membership_for_user_in_org_is_true_only_for_an_active_membership() {
        let github = StubGithub::serving(vec![
            route(
                "GET",
                "/orgs/org/teams/team/memberships/inactive",
                200,
                json!({"state": false}),
            ),
            route(
                "GET",
                "/orgs/org/teams/team/memberships/active",
                200,
                json!({"state": "active"}),
            ),
        ])
        .await;
        let context = github.pull_request_context();
        assert!(
            !fetch_membership_for_user_in_org(&context, "org", "team", &json!("inactive"))
                .await
                .unwrap()
        );
        assert!(
            fetch_membership_for_user_in_org(&context, "org", "team", &json!("active"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn fetch_membership_for_user_in_org_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required(
            "GET",
            "/orgs/org/teams/team/memberships/user",
        )])
        .await;
        assert_payment_required(
            fetch_membership_for_user_in_org(
                &github.pull_request_context(),
                "org",
                "team",
                &json!("user"),
            )
            .await,
        );
    }

    #[tokio::test]
    async fn project_list_columns_returns_the_column_ids() {
        let github = StubGithub::serving(vec![route(
            "GET",
            PROJECT_COLUMNS,
            200,
            json!([{"id": "1"}, {"id": "2"}]),
        )])
        .await;
        let columns =
            project_list_columns(&github.pull_request_context(), json!({"project_id": 1}))
                .await
                .unwrap();
        assert_eq!(columns, vec![json!("1"), json!("2")]);
    }

    #[tokio::test]
    async fn project_list_columns_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PROJECT_COLUMNS)]).await;
        assert_payment_required(
            project_list_columns(&github.pull_request_context(), json!({"project_id": 1})).await,
        );
    }

    #[tokio::test]
    async fn project_list_for_repo_returns_the_project_ids_and_names() {
        let projects = json!([{"name": "Project One", "id": 1}, {"name": "Project Two", "id": 2}]);
        let github = StubGithub::serving(vec![route(
            "GET",
            REPOSITORY_PROJECTS,
            200,
            projects.clone(),
        )])
        .await;
        let listed = project_list_for_repo(&github.pull_request_context(), repository())
            .await
            .unwrap();
        assert_eq!(Value::Array(listed), projects);
    }

    #[tokio::test]
    async fn project_list_for_repo_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", REPOSITORY_PROJECTS)]).await;
        assert_payment_required(
            project_list_for_repo(&github.pull_request_context(), repository()).await,
        );
    }

    #[tokio::test]
    async fn project_list_cards_returns_the_response() {
        let cards =
            json!([{"content_url": "testRepo/issues/1"}, {"content_url": "testRepo/issues/2"}]);
        let github =
            StubGithub::serving(vec![route("GET", COLUMN_CARDS, 200, cards.clone())]).await;
        let response = project_list_cards(&github.pull_request_context(), json!({"column_id": 1}))
            .await
            .unwrap();
        assert_eq!(response.data, cards);
    }

    #[tokio::test]
    async fn project_list_cards_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", COLUMN_CARDS)]).await;
        assert_payment_required(
            project_list_cards(&github.pull_request_context(), json!({"column_id": 1})).await,
        );
    }

    #[tokio::test]
    async fn list_collaborators_returns_the_collaborator_logins() {
        let github = StubGithub::serving(vec![route(
            "GET",
            COLLABORATORS,
            200,
            json!([{"login": "member1"}, {"login": "member2"}]),
        )])
        .await;
        let collaborators = list_collaborators(&github.pull_request_context(), repository())
            .await
            .unwrap();
        assert_eq!(collaborators, vec![json!("member1"), json!("member2")]);
    }

    #[tokio::test]
    async fn list_collaborators_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", COLLABORATORS)]).await;
        assert_payment_required(
            list_collaborators(&github.pull_request_context(), repository()).await,
        );
    }

    #[tokio::test]
    async fn fetch_all_topics_returns_the_topic_names() {
        let github = StubGithub::serving(vec![route(
            "GET",
            TOPICS,
            200,
            json!({"names": ["topic 1", "topic 2"]}),
        )])
        .await;
        let topics = fetch_all_topics(&github.pull_request_context(), repository())
            .await
            .unwrap();
        assert_eq!(topics, json!(["topic 1", "topic 2"]));
    }

    #[tokio::test]
    async fn fetch_all_topics_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", TOPICS)]).await;
        assert_payment_required(
            fetch_all_topics(&github.pull_request_context(), repository()).await,
        );
    }

    fn comparison() -> Value {
        json!({"owner": "owner", "repo": "repo", "base": "base", "head": "head"})
    }

    #[tokio::test]
    async fn compare_commits_returns_the_comparison() {
        let github = StubGithub::serving(vec![route(
            "GET",
            COMPARISON,
            200,
            json!({"files": ["file 1", "file 2"]}),
        )])
        .await;
        let compared = compare_commits(&github.pull_request_context(), comparison())
            .await
            .unwrap();
        assert_eq!(compared["files"], json!(["file 1", "file 2"]));
    }

    #[tokio::test]
    async fn compare_commits_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", COMPARISON)]).await;
        assert_payment_required(
            compare_commits(&github.pull_request_context(), comparison()).await,
        );
    }

    #[tokio::test]
    async fn check_if_merged_is_true_when_merged_and_false_when_not_found() {
        let github = StubGithub::serving(vec![
            empty_route("GET", PULL_MERGE, 204),
            not_found("GET", "/repos/owner/repo/pulls/2/merge"),
        ])
        .await;
        let context = github.pull_request_context();
        assert!(check_if_merged(&context, &json!(1)).await.unwrap());
        assert!(!check_if_merged(&context, &json!(2)).await.unwrap());
    }

    #[tokio::test]
    async fn check_if_merged_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PULL_MERGE)]).await;
        assert_payment_required(check_if_merged(&github.pull_request_context(), &json!(1)).await);
    }

    fn merge_request() -> Value {
        json!({"owner": "owner", "repo": "repo", "pull_number": 1})
    }

    #[tokio::test]
    async fn merge_pull_request_returns_the_response() {
        let github =
            StubGithub::serving(vec![route("PUT", PULL_MERGE, 200, json!("merged"))]).await;
        let response = merge_pull_request(&github.pull_request_context(), merge_request())
            .await
            .unwrap();
        assert_eq!(response.map(|merged| merged.data), Some(json!("merged")));
    }

    #[tokio::test]
    async fn merge_pull_request_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("PUT", PULL_MERGE)]).await;
        assert_payment_required(
            merge_pull_request(&github.pull_request_context(), merge_request()).await,
        );
    }

    #[tokio::test]
    async fn request_reviewers_returns_the_response() {
        let github = StubGithub::serving(vec![route(
            "POST",
            REQUESTED_REVIEWERS,
            201,
            json!("request review success"),
        )])
        .await;
        let response = request_reviewers(
            &github.pull_request_context(),
            &json!(1),
            Some(json!(["reviewer"])),
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            response.map(|requested| requested.data),
            Some(json!("request review success"))
        );
    }

    #[tokio::test]
    async fn request_reviewers_does_not_rethrow_errors() {
        let github = StubGithub::serving(vec![payment_required("POST", REQUESTED_REVIEWERS)]).await;
        let response = request_reviewers(
            &github.pull_request_context(),
            &json!(1),
            Some(json!(["reviewer"])),
            None,
        )
        .await
        .unwrap();
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn fetch_pull_request_returns_the_pull_request() {
        let github =
            StubGithub::serving(vec![route("GET", PULL_REQUEST, 200, json!({"number": 12}))]).await;
        let response = fetch_pull_request(&github.pull_request_context(), &json!(12))
            .await
            .unwrap();
        assert_eq!(response.data, json!({"number": 12}));
    }

    #[tokio::test]
    async fn fetch_pull_request_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PULL_REQUEST)]).await;
        assert_payment_required(
            fetch_pull_request(&github.pull_request_context(), &json!(12)).await,
        );
    }

    #[tokio::test]
    async fn list_pull_requests_returns_the_response() {
        let github = StubGithub::serving(vec![route("GET", PULL_REQUESTS, 200, json!([]))]).await;
        let response = list_pull_requests(&github.pull_request_context())
            .await
            .unwrap();
        assert_eq!(response.data, json!([]));
    }

    #[tokio::test]
    async fn list_pull_requests_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PULL_REQUESTS)]).await;
        assert_payment_required(list_pull_requests(&github.pull_request_context()).await);
    }

    #[tokio::test]
    async fn list_reviews_returns_the_reviews() {
        let github = StubGithub::serving(vec![route(
            "GET",
            PULL_REVIEWS,
            200,
            json!(["review 1", "review 2"]),
        )])
        .await;
        let reviews = list_reviews(&github.pull_request_context(), &json!(1))
            .await
            .unwrap();
        assert_eq!(reviews, vec![json!("review 1"), json!("review 2")]);
    }

    #[tokio::test]
    async fn list_reviews_collects_every_page() {
        let github = StubGithub::serving(two_pages(
            PULL_REVIEWS,
            json!(["review 1"]),
            json!(["review 2"]),
        ))
        .await;
        let reviews = list_reviews(&github.pull_request_context(), &json!(1))
            .await
            .unwrap();
        assert_eq!(reviews, vec![json!("review 1"), json!("review 2")]);
    }

    #[tokio::test]
    async fn list_reviews_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PULL_REVIEWS)]).await;
        assert_payment_required(list_reviews(&github.pull_request_context(), &json!(1)).await);
    }

    #[tokio::test]
    async fn list_commits_returns_message_date_author_and_committer() {
        let date = "2024-01-02T03:04:05Z";
        let github = StubGithub::serving(vec![route(
            "GET",
            PULL_COMMITS,
            200,
            json!([{"commit": {
                "author": {"date": date, "name": "Monalisa Octocat", "email": "support@github.com"},
                "committer": {"date": date, "name": "Valdis Ferdinand", "email": "valdis@github.com"},
                "message": "fix: this",
            }}]),
        )])
        .await;
        let commits = list_commits(&github.pull_request_context(), &json!(1))
            .await
            .unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0]["date"], json!(date));
        assert_eq!(commits[0]["message"], json!("fix: this"));
        assert_eq!(commits[0]["author"]["email"], json!("support@github.com"));
        assert_eq!(commits[0]["committer"]["email"], json!("valdis@github.com"));
    }

    #[tokio::test]
    async fn list_commits_collects_every_page() {
        let commit =
            |message: &str| json!({"commit": {"author": {"date": "d"}, "message": message}});
        let github = StubGithub::serving(two_pages(
            PULL_COMMITS,
            json!([commit("one")]),
            json!([commit("two")]),
        ))
        .await;
        let commits = list_commits(&github.pull_request_context(), &json!(1))
            .await
            .unwrap();
        let messages: Vec<&Value> = commits.iter().map(|summary| &summary["message"]).collect();
        assert_eq!(messages, vec![&json!("one"), &json!("two")]);
    }

    #[tokio::test]
    async fn list_commits_rethrows_errors() {
        let github = StubGithub::serving(vec![payment_required("GET", PULL_COMMITS)]).await;
        assert_payment_required(list_commits(&github.pull_request_context(), &json!(1)).await);
    }
}
