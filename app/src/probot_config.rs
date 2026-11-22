use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::js;
use crate::yaml;
use fancy_regex::Regex;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;

const SUPPORTED_FILE_EXTENSIONS: [&str; 3] = ["json", "yml", "yaml"];
const CONTENTS_ROUTE: &str = "GET /repos/{owner}/{repo}/contents/{path}";

static EXTENDS_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:([a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38})/)?([-_.\w\d]+)(?::([-_./\w\d]+\.ya?ml))?$").ok()
});

#[derive(Clone)]
struct ConfigFile {
    owner: String,
    repo: String,
    path: String,
    url: String,
    config: Option<Value>,
}

fn plugin_error(message: &str) -> JsError {
    JsError::error(format!("[@probot/octokit-plugin-config] {message}"))
}

async fn get_config_file(
    context: &Context,
    owner: &str,
    repo: &str,
    path: &str,
    reference: Option<&str>,
) -> JsResult<ConfigFile> {
    let file_extension = path.rsplit('.').next().unwrap_or_default().to_lowercase();
    if !SUPPORTED_FILE_EXTENSIONS.contains(&file_extension.as_str()) {
        return Err(plugin_error(&format!(
            ".{file_extension} extension is not support for configuration (path: \"{path}\")"
        )));
    }
    let mut params = json!({"owner": owner, "repo": repo, "path": path});
    if let Some(branch) = reference {
        params["ref"] = Value::String(branch.to_string());
    }
    let url = context.github.endpoint_url(CONTENTS_ROUTE, &params)?;
    let empty = ConfigFile {
        owner: owner.to_string(),
        repo: repo.to_string(),
        path: path.to_string(),
        url: url.clone(),
        config: None,
    };
    let response = match context.github.request_raw(CONTENTS_ROUTE, params).await {
        Ok(response) => response,
        Err(error) if error.status == Some(404) => return Ok(empty),
        Err(error) => return Err(error),
    };
    if response.header("content-type") == Some("application/json; charset=utf-8") {
        return Err(plugin_error(&format!(
            "{url} exists, but is either a directory or a submodule. Ignoring."
        )));
    }
    if file_extension == "json" {
        if response.data.is_string() {
            return Err(plugin_error(&format!(
                "Configuration could not be parsed from {url} (invalid JSON)"
            )));
        }
        return Ok(ConfigFile {
            config: Some(response.data),
            ..empty
        });
    }
    let text = js::to_js_string(Some(&response.data));
    let parsed = yaml::parse_like_js_yaml_4(&text).map_err(|error| {
        let reason = if error.message.contains("unknown tag") {
            "unsafe YAML"
        } else {
            "invalid YAML"
        };
        plugin_error(&format!(
            "Configuration could not be parsed from {url} ({reason})"
        ))
    })?;
    let config = match parsed {
        Some(value) if js::is_truthy(&value) => value,
        _ => Value::Object(Map::new()),
    };
    if config.is_string() {
        return Err(plugin_error(&format!(
            "Configuration could not be parsed from {url} (YAML is not an object)"
        )));
    }
    Ok(ConfigFile {
        config: Some(config),
        ..empty
    })
}

struct ExtendsTarget {
    owner: String,
    repo: String,
    path: String,
}

fn extends_to_target(
    owner: &str,
    path: &str,
    url: &str,
    extends_value: &Value,
) -> JsResult<ExtendsTarget> {
    let Value::String(extends_text) = extends_value else {
        return Err(plugin_error(&format!(
            "Invalid value {} for _extends in {url}",
            js::stringify(extends_value)
        )));
    };
    let captures = EXTENDS_PATTERN
        .as_ref()
        .and_then(|pattern| pattern.captures(extends_text).ok().flatten())
        .ok_or_else(|| {
            plugin_error(&format!(
                "Invalid value \"{extends_text}\" for _extends in {url}"
            ))
        })?;
    let group = |index: usize| captures.get(index).map(|found| found.as_str().to_string());
    Ok(ExtendsTarget {
        owner: group(1).unwrap_or_else(|| owner.to_string()),
        repo: group(2).unwrap_or_default(),
        path: group(3).unwrap_or_else(|| path.to_string()),
    })
}

fn take_extends(file: &mut ConfigFile) -> Option<Value> {
    let Some(Value::Object(config)) = file.config.as_mut() else {
        return None;
    };
    if !config.get("_extends").is_some_and(js::is_truthy) {
        return None;
    }
    config.shift_remove("_extends")
}

async fn get_config_files(
    context: &Context,
    owner: &str,
    repo: &str,
    path: &str,
) -> JsResult<Vec<ConfigFile>> {
    let requested = get_config_file(context, owner, repo, path, None).await?;
    let mut files = vec![requested];
    if files[0].config.is_none() {
        if repo == ".github" {
            return Ok(files);
        }
        files.push(get_config_file(context, owner, ".github", path, None).await?);
    }
    let last_index = files.len() - 1;
    let Some(extends_value) = take_extends(&mut files[last_index]) else {
        return Ok(files);
    };
    let mut target = extends_to_target(owner, path, &files[last_index].url, &extends_value)?;
    loop {
        let mut extended =
            get_config_file(context, &target.owner, &target.repo, &target.path, None).await?;
        let next_extends = take_extends(&mut extended);
        let extended_url = extended.url.clone();
        files.push(extended);
        let Some(extends_value) = next_extends else {
            return Ok(files);
        };
        target = extends_to_target(owner, path, &extended_url, &extends_value)?;
        if let Some(already_loaded) = files.iter().find(|file| {
            file.owner == target.owner && file.repo == target.repo && file.path == target.path
        }) {
            return Err(plugin_error(&format!(
                "Recursion detected. Ignoring  \"_extends: undefined\" from {extended_url} because {} was already loaded.",
                already_loaded.url
            )));
        }
    }
}

fn is_mergeable_object(value: &Value) -> bool {
    value.is_object() || value.is_array()
}

fn deepmerge_owned(target: Value, source: Value) -> Value {
    match (target, source) {
        (Value::Array(mut target_items), Value::Array(source_items)) => {
            target_items.extend(source_items);
            Value::Array(target_items)
        }
        (Value::Object(mut destination), Value::Object(source_map)) => {
            for (key, source_value) in source_map {
                let merged = match destination.get_mut(&key) {
                    Some(target_value) if is_mergeable_object(&source_value) => {
                        deepmerge_owned(target_value.take(), source_value)
                    }
                    _ => source_value,
                };
                destination.insert(key, merged);
            }
            Value::Object(destination)
        }
        (_, other) => other,
    }
}

pub async fn load(
    context: &Context,
    file_name: &str,
    default_config: Option<Value>,
) -> JsResult<Option<Value>> {
    let (owner, repo) = context.repo()?;
    let path = format!(".github/{}", file_name.trim_start_matches('/'));
    let files = get_config_files(context, &owner, &repo, &path).await?;
    let has_default = default_config.as_ref().is_some_and(js::is_truthy);
    if !has_default && files.iter().all(|file| file.config.is_none()) {
        return Ok(None);
    }
    let initial = match default_config {
        Some(default_value) if js::is_truthy(&default_value) => default_value,
        _ => Value::Object(Map::new()),
    };
    let merged = files
        .into_iter()
        .rev()
        .filter_map(|file| file.config.filter(js::is_truthy))
        .fold(initial, deepmerge_owned);
    Ok(Some(merged))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepmerge_concatenates_arrays_like_the_deepmerge_package() {
        let base = json!({"mergeable": [{"when": "a"}], "version": 2});
        let repo = json!({"mergeable": [{"when": "b"}]});
        assert_eq!(
            deepmerge_owned(base, repo),
            json!({"mergeable": [{"when": "a"}, {"when": "b"}], "version": 2})
        );
    }

    #[test]
    fn extends_value_with_owner_repo_and_path_is_parsed() {
        let target = extends_to_target(
            "acme",
            ".github/mergeable.yml",
            "url",
            &json!("other/shared:configs/base.yml"),
        )
        .unwrap();
        assert_eq!(
            (
                target.owner.as_str(),
                target.repo.as_str(),
                target.path.as_str()
            ),
            ("other", "shared", "configs/base.yml")
        );
        let target =
            extends_to_target("acme", ".github/mergeable.yml", "url", &json!("shared")).unwrap();
        assert_eq!(
            (
                target.owner.as_str(),
                target.repo.as_str(),
                target.path.as_str()
            ),
            ("acme", "shared", ".github/mergeable.yml")
        );
    }
}
