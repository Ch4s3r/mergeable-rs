use crate::actions::{self, Action};
use crate::error::{JsError, JsResult};
use crate::filters::{self, Filter};
use crate::js;
use crate::validators::{self, Validator};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
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

fn for_each_entry<'list>(
    list: Option<&'list Value>,
    list_description: &str,
) -> JsResult<&'list [Value]> {
    match list {
        Some(Value::Array(items)) => Ok(items),
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

fn register_entries<'entry, Module: ?Sized>(
    registered: &Mutex<HashMap<String, Arc<Module>>>,
    entries: impl IntoIterator<Item = &'entry Value>,
    kind: &str,
    create: impl Fn(&str) -> Option<Arc<Module>>,
) -> JsResult<()> {
    for entry in entries {
        let key = entry_key(entry)?;
        let mut registered = registered
            .lock()
            .map_err(|_| JsError::error("registry poisoned"))?;
        if let Entry::Vacant(slot) = registered.entry(key) {
            let module = create(slot.key()).ok_or_else(|| module_not_found(kind, slot.key()))?;
            slot.insert(module);
        }
    }
    Ok(())
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
        register_entries(
            &self.filters,
            for_each_entry(rule.get("filter"), "rule.filter")?,
            "filters",
            filters::create,
        )
    }

    pub fn register_validators(&self, rule: &Value) -> JsResult<()> {
        register_entries(
            &self.validators,
            for_each_entry(rule.get("validate"), "rule.validate")?,
            "validators",
            validators::create,
        )
    }

    pub fn register_actions(&self, rule: &Value) -> JsResult<()> {
        let possible_actions = ["pass", "fail", "error"]
            .into_iter()
            .filter_map(|outcome| rule.get(outcome).filter(|actions| js::is_truthy(actions)))
            .flat_map(|actions| match actions {
                Value::Array(items) => items.as_slice(),
                single => std::slice::from_ref(single),
            });
        register_entries(&self.actions, possible_actions, "actions", actions::create)
    }

    pub fn register_all(&self, settings: &Value) -> JsResult<()> {
        for rule in for_each_entry(Some(settings), "settings")? {
            self.register_filters(rule)
                .map_err(|error| JsError::error(format!("Filters have thrown {error}")))?;
            self.register_validators(rule)
                .map_err(|error| JsError::error(format!("Validators have thrown {error}")))?;
            self.register_actions(rule)
                .map_err(|error| JsError::error(format!("Actions have thrown {error}")))?;
        }
        Ok(())
    }
}
