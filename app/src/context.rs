use crate::error::{Error, Result};
use crate::github::client::InstallationClient;
use crate::json_value;
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use std::ops::Deref;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SharedPayload(Arc<Value>);

impl SharedPayload {
    pub fn to_mut(&mut self) -> &mut Value {
        Arc::make_mut(&mut self.0)
    }
}

impl Deref for SharedPayload {
    type Target = Value;

    fn deref(&self) -> &Value {
        &self.0
    }
}

impl From<Value> for SharedPayload {
    fn from(payload: Value) -> Self {
        SharedPayload(Arc::new(payload))
    }
}

impl Serialize for SharedPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

#[derive(Clone)]
pub struct Context {
    pub event_id: String,
    pub event_name: String,
    pub payload: SharedPayload,
    pub github: InstallationClient,
    pub global_settings: Option<Arc<Value>>,
}

impl Context {
    pub fn new(
        event_id: String,
        event_name: String,
        payload: impl Into<SharedPayload>,
        github: InstallationClient,
    ) -> Self {
        Context {
            event_id,
            event_name,
            payload: payload.into(),
            github,
            global_settings: None,
        }
    }

    #[must_use]
    pub fn action(&self) -> Option<&Value> {
        self.payload.get("action")
    }

    #[must_use]
    pub fn event_with_action(&self) -> String {
        format!(
            "{}.{}",
            self.event_name,
            json_value::to_display_string(self.action())
        )
    }

    #[must_use]
    pub fn event_label(&self) -> String {
        if json_value::truthy(self.action()) {
            self.event_with_action()
        } else {
            self.event_name.clone()
        }
    }

    pub fn repo(&self) -> Result<(String, String)> {
        let repository = self
            .payload
            .get("repository")
            .filter(|repository| repository.is_object())
            .ok_or_else(|| {
                Error::general("context.repo() is not supported for this webhook event.")
            })?;
        Ok((
            json_value::read_string(repository, &["owner", "login"])?,
            json_value::read_string(repository, &["name"])?,
        ))
    }

    pub fn repo_params(&self, extra: Value) -> Result<Value> {
        let (owner, repo) = self.repo()?;
        let mut params = Map::new();
        params.insert("owner".to_string(), Value::String(owner));
        params.insert("repo".to_string(), Value::String(repo));
        if let Value::Object(extra_fields) = extra {
            params.extend(extra_fields);
        }
        Ok(Value::Object(params))
    }

    #[must_use]
    pub fn repository_full_name(&self) -> String {
        json_value::to_display_string(json_value::get_path(
            &self.payload,
            &["repository", "full_name"],
        ))
    }

    pub fn global_setting(&self, key: &str) -> Result<Option<&Value>> {
        match &self.global_settings {
            None => Err(Error::missing_property(key)),
            Some(settings) => Ok(settings.get(key)),
        }
    }
}
