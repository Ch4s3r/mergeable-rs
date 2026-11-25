use crate::actions::{self, Action};
use crate::error::{Error, Result};
use crate::filters::{self, Filter};
use crate::json_value;
use crate::results::Status;
use crate::validators::{self, Validator};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, PoisonError, RwLock};

type Modules<Module> = RwLock<HashMap<String, Arc<Module>>>;

#[derive(Default)]
pub struct Registry {
    filters: Modules<dyn Filter>,
    validators: Modules<dyn Validator>,
    actions: Modules<dyn Action>,
}

fn module_not_found(kind: &str, key: &str) -> Error {
    Error::general(format!("Cannot find module './{kind}/{key}'"))
}

fn rule_list<'list>(list: Option<&'list Value>, list_description: &str) -> Result<&'list [Value]> {
    match list {
        Some(Value::Array(items)) => Ok(items),
        None => Err(Error::missing_property("forEach")),
        Some(Value::Null) => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'forEach')",
        )),
        Some(_) => Err(Error::type_mismatch(format!(
            "{list_description}.forEach is not a function"
        ))),
    }
}

fn lookup<Module: ?Sized>(registered: &Modules<Module>, key: &str) -> Option<Arc<Module>> {
    registered
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(key)
        .cloned()
}

fn register_entries<'entry, Module: ?Sized>(
    registered: &Modules<Module>,
    entries: impl IntoIterator<Item = &'entry Value>,
    kind: &str,
    create: impl Fn(&str) -> Option<Arc<Module>>,
) -> Result<()> {
    let mut registered = registered.write().unwrap_or_else(PoisonError::into_inner);
    for entry in entries {
        let key = json_value::to_display_string(json_value::read(Some(entry), "do")?);
        if let Entry::Vacant(slot) = registered.entry(key) {
            let module = create(slot.key()).ok_or_else(|| module_not_found(kind, slot.key()))?;
            slot.insert(module);
        }
    }
    Ok(())
}

impl Registry {
    pub fn filter(&self, key: &str) -> Option<Arc<dyn Filter>> {
        lookup(&self.filters, key)
    }

    pub fn validator(&self, key: &str) -> Option<Arc<dyn Validator>> {
        lookup(&self.validators, key)
    }

    pub fn action(&self, key: &str) -> Option<Arc<dyn Action>> {
        lookup(&self.actions, key)
    }

    pub fn register_filters(&self, rule: &Value) -> Result<()> {
        if !json_value::truthy(rule.get("filter")) {
            return Ok(());
        }
        register_entries(
            &self.filters,
            rule_list(rule.get("filter"), "rule.filter")?,
            "filters",
            filters::create,
        )
    }

    pub fn register_validators(&self, rule: &Value) -> Result<()> {
        register_entries(
            &self.validators,
            rule_list(rule.get("validate"), "rule.validate")?,
            "validators",
            validators::create,
        )
    }

    pub fn register_actions(&self, rule: &Value) -> Result<()> {
        let outcome_actions = Status::ALL
            .into_iter()
            .filter_map(|outcome| {
                rule.get(outcome.as_str())
                    .filter(|actions| json_value::is_truthy(actions))
            })
            .flat_map(|actions| match actions {
                Value::Array(items) => items.as_slice(),
                single => std::slice::from_ref(single),
            });
        register_entries(&self.actions, outcome_actions, "actions", actions::create)
    }

    pub fn register_all(&self, settings: &Value) -> Result<()> {
        for rule in rule_list(Some(settings), "settings")? {
            self.register_filters(rule)
                .map_err(|error| Error::general(format!("Filters have thrown {error}")))?;
            self.register_validators(rule)
                .map_err(|error| Error::general(format!("Validators have thrown {error}")))?;
            self.register_actions(rule)
                .map_err(|error| Error::general(format!("Actions have thrown {error}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl Registry {
    pub fn replace_validator(&self, key: &str, validator: Arc<dyn Validator>) {
        self.validators
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_string(), validator);
    }

    pub fn replace_action(&self, key: &str, action: Arc<dyn Action>) {
        self.actions
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_string(), action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registering_a_rule_makes_its_modules_available() -> Result<()> {
        let registry = Registry::default();
        registry.register_all(&json!([{
            "filter": [{"do": "author"}],
            "validate": [{"do": "title"}],
            "pass": [{"do": "comment"}],
            "fail": {"do": "labels"}
        }]))?;
        assert!(registry.filter("author").is_some());
        assert!(registry.validator("title").is_some());
        assert!(registry.action("comment").is_some());
        assert!(registry.action("labels").is_some());
        assert!(registry.action("merge").is_none());
        Ok(())
    }

    #[test]
    fn unknown_module_names_the_kind_that_failed() {
        let failure = Registry::default()
            .register_all(&json!([{"validate": [{"do": "nonexistent"}]}]))
            .err()
            .map(|error| error.to_string());
        assert_eq!(
            failure.as_deref(),
            Some(
                "Error: Validators have thrown Error: Cannot find module './validators/nonexistent'"
            )
        );
    }
}
