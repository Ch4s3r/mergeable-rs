use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::github::api;
use crate::js;
use crate::validators::options_processor::git_pattern;
use crate::validators::options_processor::teams;
use serde_json::{Value, json};

const OWNER_FILE_PATH: &str = ".github/CODEOWNERS";

pub struct SeparatedOwners {
    pub teams: Vec<Value>,
    pub individuals: Vec<Value>,
}

pub fn separate_teams_and_individuals(owners: &[String]) -> SeparatedOwners {
    let mut separated = SeparatedOwners {
        teams: Vec::new(),
        individuals: Vec::new(),
    };
    for owner in owners {
        if owner.contains('/') {
            separated.teams.push(Value::String(owner.clone()));
        } else {
            separated.individuals.push(Value::String(owner.clone()));
        }
    }
    separated
}

fn extract_user_ids(owners: Vec<String>) -> JsResult<Vec<String>> {
    owners
        .into_iter()
        .map(|owner| match owner.strip_prefix('@') {
            Some(user_id) => Ok(user_id.to_string()),
            None => Err(JsError::type_error("EMAIL_REGEX.test is not a function")),
        })
        .collect()
}

async fn retrieve_codeowners(context: &Context) -> JsResult<Option<String>> {
    let (owner, repo) = context.repo()?;
    if context.event_name == "pull_request" || context.event_name == "pull_request_review" {
        let pull_number = js::read_path(&context.payload, &["pull_request", "number"])?
            .cloned()
            .unwrap_or(Value::Null);
        let files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;
        let mut modified_files = Vec::new();
        for changed_file in &files {
            let status = js::read(Some(changed_file), "status")?;
            if matches!(js::as_str(status), Some("modified") | Some("added")) {
                modified_files.push(
                    js::read(Some(changed_file), "filename")?
                        .cloned()
                        .unwrap_or(Value::Null),
                );
            }
        }
        if modified_files
            .iter()
            .any(|filename| filename.as_str() == Some(OWNER_FILE_PATH))
        {
            let head_sha = js::read_path(&context.payload, &["pull_request", "head", "sha"])?
                .cloned()
                .unwrap_or(Value::Null);
            let call_parameters =
                json!({"owner": owner, "repo": repo, "path": OWNER_FILE_PATH, "ref": head_sha});
            return api::get_content(context, call_parameters).await;
        }
    }
    api::get_content(
        context,
        context.repo_params(json!({"path": OWNER_FILE_PATH}))?,
    )
    .await
}

fn union(first: Vec<Value>, second: Vec<Value>) -> Vec<Value> {
    let mut combined: Vec<Value> = Vec::new();
    for value in first.into_iter().chain(second) {
        if !combined.contains(&value) {
            combined.push(value);
        }
    }
    combined
}

pub async fn process(payload: &Value, context: &Context) -> JsResult<Vec<Value>> {
    let Some(codeowners) = retrieve_codeowners(context).await? else {
        return Ok(Vec::new());
    };
    let owner_file = git_pattern::parse_owner_file(&codeowners);

    let base_sha = js::read_path(payload, &["base", "sha"])?
        .cloned()
        .unwrap_or(Value::Null);
    let head_sha = js::read_path(payload, &["head", "sha"])?
        .cloned()
        .unwrap_or(Value::Null);
    let compare = api::compare_commits(
        context,
        context.repo_params(json!({"base": base_sha, "head": head_sha}))?,
    )
    .await?;
    let paths: Vec<String> = match js::read(Some(&compare), "files")? {
        Some(Value::Array(files)) => files
            .iter()
            .map(|changed_file| Ok(js::to_js_string(js::read(Some(changed_file), "filename")?)))
            .collect::<JsResult<_>>()?,
        None => return Err(JsError::reading_property_of_undefined("map")),
        Some(_) => return Err(JsError::type_error("compare.files.map is not a function")),
    };

    let mut owner_list = Vec::new();
    for path in &paths {
        let path_owners = owner_file.owners_for(path)?;
        if !path_owners.is_empty() {
            owner_list = path_owners;
        }
    }

    let owner_list = extract_user_ids(owner_list)?;
    let separated = separate_teams_and_individuals(&owner_list);
    let team_members = if separated.teams.is_empty() {
        Vec::new()
    } else {
        teams::extract_team_memberships(context, &separated.teams, &separated.individuals).await?
    };
    Ok(union(team_members, separated.individuals))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_with_a_slash_are_teams() {
        let separated =
            separate_teams_and_individuals(&["org/team".to_string(), "bob".to_string()]);
        assert_eq!(separated.teams, vec![json!("org/team")]);
        assert_eq!(separated.individuals, vec![json!("bob")]);
    }

    #[test]
    fn at_prefix_is_stripped_and_email_owners_hit_the_node_type_error() {
        assert_eq!(
            extract_user_ids(vec!["@bob".to_string()]).unwrap(),
            vec!["bob"]
        );
        let error = extract_user_ids(vec!["bob@example.com".to_string()]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: EMAIL_REGEX.test is not a function"
        );
    }

    #[test]
    fn union_keeps_first_occurrence_order() {
        assert_eq!(
            union(vec![json!("a"), json!("b")], vec![json!("b"), json!("c")]),
            vec![json!("a"), json!("b"), json!("c")]
        );
    }
}
