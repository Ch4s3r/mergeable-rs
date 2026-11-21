use crate::error::{JsError, JsResult};
use crate::github::client::InstallationClient;
use crate::js;
use serde_json::{Map, Value};

#[derive(Clone)]
pub struct Context {
    pub event_id: String,
    pub event_name: String,
    pub payload: Value,
    pub github: InstallationClient,
    pub global_settings: Option<Value>,
}

impl Context {
    pub fn new(
        event_id: String,
        event_name: String,
        payload: Value,
        github: InstallationClient,
    ) -> Self {
        Context {
            event_id,
            event_name,
            payload,
            github,
            global_settings: None,
        }
    }

    pub fn action(&self) -> Option<&Value> {
        self.payload.get("action")
    }

    pub fn event_with_action(&self) -> String {
        format!("{}.{}", self.event_name, js::to_js_string(self.action()))
    }

    pub fn event_label(&self) -> String {
        if js::truthy(self.action()) {
            self.event_with_action()
        } else {
            self.event_name.clone()
        }
    }

    pub fn repo(&self) -> JsResult<(String, String)> {
        let repository = self
            .payload
            .get("repository")
            .filter(|repository| repository.is_object())
            .ok_or_else(|| {
                JsError::error("context.repo() is not supported for this webhook event.")
            })?;
        Ok((
            js::read_string(repository, &["owner", "login"])?,
            js::read_string(repository, &["name"])?,
        ))
    }

    pub fn repo_params(&self, extra: Value) -> JsResult<Value> {
        let (owner, repo) = self.repo()?;
        let mut params = Map::new();
        params.insert("owner".to_string(), Value::String(owner));
        params.insert("repo".to_string(), Value::String(repo));
        if let Value::Object(extra_fields) = extra {
            for (key, value) in extra_fields {
                params.insert(key, value);
            }
        }
        Ok(Value::Object(params))
    }

    pub fn repository_full_name(&self) -> String {
        js::to_js_string(js::get_path(&self.payload, &["repository", "full_name"]))
    }

    pub fn global_setting(&self, key: &str) -> JsResult<Option<&Value>> {
        match &self.global_settings {
            None => Err(JsError::reading_property_of_undefined(key)),
            Some(settings) => Ok(settings.get(key)),
        }
    }
}
