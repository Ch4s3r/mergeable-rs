use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::api;
use crate::json_value;
use crate::validators::options_processor::git_pattern;
use crate::validators::options_processor::{teams, unique_values};
use serde_json::{Value, json};

const OWNER_FILE_PATH: &str = ".github/CODEOWNERS";

pub struct SeparatedOwners {
    pub teams: Vec<Value>,
    pub individuals: Vec<Value>,
}

#[must_use]
pub fn separate_teams_and_individuals(owners: Vec<String>) -> SeparatedOwners {
    let mut separated = SeparatedOwners {
        teams: Vec::new(),
        individuals: Vec::new(),
    };
    for owner in owners {
        if owner.contains('/') {
            separated.teams.push(Value::String(owner));
        } else {
            separated.individuals.push(Value::String(owner));
        }
    }
    separated
}

fn extract_user_ids(owners: &[String]) -> Result<Vec<String>> {
    owners
        .iter()
        .map(|owner| match owner.strip_prefix('@') {
            Some(user_id) => Ok(user_id.to_string()),
            None => Err(Error::type_mismatch("EMAIL_REGEX.test is not a function")),
        })
        .collect()
}

async fn retrieve_codeowners(context: &Context) -> Result<Option<String>> {
    let repository = context.repository_name()?;
    if context.event_name == "pull_request" || context.event_name == "pull_request_review" {
        let pull_number = json_value::read_path(&context.payload, &["pull_request", "number"])?
            .unwrap_or(&Value::Null);
        let files = api::list_files(
            context,
            context.repo_params(json!({"pull_number": pull_number}))?,
        )
        .await?;
        let mut is_owner_file_modified = false;
        for changed_file in &files {
            let status = json_value::read(Some(changed_file), "status")?;
            if matches!(json_value::as_str(status), Some("modified" | "added")) {
                let filename = json_value::read(Some(changed_file), "filename")?;
                is_owner_file_modified |= json_value::as_str(filename) == Some(OWNER_FILE_PATH);
            }
        }
        if is_owner_file_modified {
            let head_sha =
                json_value::read_path(&context.payload, &["pull_request", "head", "sha"])?
                    .unwrap_or(&Value::Null);
            let call_parameters = json!({"owner": &repository.owner, "repo": &repository.name, "path": OWNER_FILE_PATH, "ref": head_sha});
            return api::fetch_content(context, call_parameters).await;
        }
    }
    api::fetch_content(
        context,
        context.repo_params(json!({"path": OWNER_FILE_PATH}))?,
    )
    .await
}

fn union(first: Vec<Value>, second: Vec<Value>) -> Vec<Value> {
    unique_values(first.into_iter().chain(second).collect())
}

pub async fn process(payload: &Value, context: &Context) -> Result<Vec<Value>> {
    let Some(codeowners) = retrieve_codeowners(context).await? else {
        return Ok(Vec::new());
    };
    let owner_file = git_pattern::parse_owner_file(&codeowners);

    let base_sha = json_value::read_path(payload, &["base", "sha"])?.unwrap_or(&Value::Null);
    let head_sha = json_value::read_path(payload, &["head", "sha"])?.unwrap_or(&Value::Null);
    let compare = api::compare_commits(
        context,
        context.repo_params(json!({"base": base_sha, "head": head_sha}))?,
    )
    .await?;
    let paths: Vec<String> = match json_value::read(Some(&compare), "files")? {
        Some(Value::Array(files)) => files
            .iter()
            .map(|changed_file| {
                Ok(json_value::to_display_string(json_value::read(
                    Some(changed_file),
                    "filename",
                )?))
            })
            .collect::<Result<_>>()?,
        None => return Err(Error::missing_property("map")),
        Some(_) => return Err(Error::type_mismatch("compare.files.map is not a function")),
    };

    let mut owner_list: &[String] = &[];
    for path in &paths {
        let path_owners = owner_file.owners_for(path)?;
        if !path_owners.is_empty() {
            owner_list = path_owners;
        }
    }

    let owner_list = extract_user_ids(owner_list)?;
    let separated = separate_teams_and_individuals(owner_list);
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
            separate_teams_and_individuals(vec!["org/team".to_string(), "bob".to_string()]);
        assert_eq!(separated.teams, vec![json!("org/team")]);
        assert_eq!(separated.individuals, vec![json!("bob")]);
    }

    #[test]
    fn at_prefix_is_stripped_and_email_owners_fail_with_a_type_error() {
        assert_eq!(
            extract_user_ids(&["@bob".to_string()]).unwrap(),
            vec!["bob"]
        );
        let error = extract_user_ids(&["bob@example.com".to_string()]).unwrap_err();
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
