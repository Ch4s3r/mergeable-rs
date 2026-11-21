use crate::actions::{self, Action};
use crate::error::{JsError, JsResult};
use crate::filters::{self, Filter};
use crate::js;
use crate::validators::{self, Validator};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Registry {
    filters: Mutex<HashMap<String, Arc<dyn Filter>>>,
    validators: Mutex<HashMap<String, Arc<dyn Validator>>>,
    actions: Mutex<HashMap<String, Arc<dyn Action>>>,
}

fn module_not_found(kind: &str, key: &str) -> JsError {
    JsError::error(format!("Cannot find module './{kind}/{key}'"))
}

fn for_each_entry(list: Option<&Value>, list_description: &str) -> JsResult<Vec<Value>> {
    match list {
        Some(Value::Array(items)) => Ok(items.clone()),
        None => Err(JsError::reading_property_of_undefined("forEach")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'forEach')",
        )),
        Some(_) => Err(JsError::type_error(format!(
            "{list_description}.forEach is not a function"
        ))),
    }
}

fn entry_key(entry: &Value) -> JsResult<String> {
    Ok(js::to_js_string(js::read(Some(entry), "do")?))
}

impl Registry {
    pub fn filter(&self, key: &str) -> Option<Arc<dyn Filter>> {
        self.filters.lock().ok()?.get(key).cloned()
    }

    pub fn validator(&self, key: &str) -> Option<Arc<dyn Validator>> {
        self.validators.lock().ok()?.get(key).cloned()
    }

    pub fn action(&self, key: &str) -> Option<Arc<dyn Action>> {
        self.actions.lock().ok()?.get(key).cloned()
    }

    pub fn register_filters(&self, rule: &Value) -> JsResult<()> {
        if !js::truthy(rule.get("filter")) {
            return Ok(());
        }
        for entry in for_each_entry(rule.get("filter"), "rule.filter")? {
            let key = entry_key(&entry)?;
            let mut registered = self
                .filters
                .lock()
                .map_err(|_| JsError::error("registry poisoned"))?;
            if let std::collections::hash_map::Entry::Vacant(slot) = registered.entry(key.clone()) {
                slot.insert(
                    filters::create(&key).ok_or_else(|| module_not_found("filters", &key))?,
                );
            }
        }
        Ok(())
    }

    pub fn register_validators(&self, rule: &Value) -> JsResult<()> {
        for entry in for_each_entry(rule.get("validate"), "rule.validate")? {
            let key = entry_key(&entry)?;
            let mut registered = self
                .validators
                .lock()
                .map_err(|_| JsError::error("registry poisoned"))?;
            if let std::collections::hash_map::Entry::Vacant(slot) = registered.entry(key.clone()) {
                slot.insert(
                    validators::create(&key).ok_or_else(|| module_not_found("validators", &key))?,
                );
            }
        }
        Ok(())
    }

    pub fn register_actions(&self, rule: &Value) -> JsResult<()> {
        let mut possible_actions: Vec<Value> = Vec::new();
        for outcome in ["pass", "fail", "error"] {
            if let Some(actions) = rule.get(outcome).filter(|actions| js::is_truthy(actions)) {
                possible_actions = js::concat(&possible_actions, actions);
            }
        }
        for entry in possible_actions {
            let key = entry_key(&entry)?;
            let mut registered = self
                .actions
                .lock()
                .map_err(|_| JsError::error("registry poisoned"))?;
            if let std::collections::hash_map::Entry::Vacant(slot) = registered.entry(key.clone()) {
                slot.insert(
                    actions::create(&key).ok_or_else(|| module_not_found("actions", &key))?,
                );
            }
        }
        Ok(())
    }

    pub fn register_all(&self, settings: &Value) -> JsResult<()> {
        for rule in for_each_entry(Some(settings), "settings")? {
            self.register_filters(&rule)
                .map_err(|error| JsError::error(format!("Filters have thrown {error}")))?;
            self.register_validators(&rule)
                .map_err(|error| JsError::error(format!("Validators have thrown {error}")))?;
            self.register_actions(&rule)
                .map_err(|error| JsError::error(format!("Actions have thrown {error}")))?;
        }
        Ok(())
    }
}
