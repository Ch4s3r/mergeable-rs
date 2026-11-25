use crate::context::Context;
use crate::error::Result;
use crate::event_aware;
use crate::filters::options_processor::name::ProcessedOption;
use crate::json_value;
use crate::results::Status;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WantedVisibility {
    Public,
    Private,
    Any,
}

impl WantedVisibility {
    fn from_setting(setting: Option<&Value>) -> Self {
        match setting.and_then(Value::as_str) {
            Some("public") => WantedVisibility::Public,
            Some("private") => WantedVisibility::Private,
            _ => WantedVisibility::Any,
        }
    }

    fn accepts(self, repository_is_private: bool) -> bool {
        match self {
            WantedVisibility::Public => !repository_is_private,
            WantedVisibility::Private => repository_is_private,
            WantedVisibility::Any => true,
        }
    }
}

pub fn process(context: &Context, settings: &Value) -> Result<ProcessedOption> {
    let payload = event_aware::payload(context);
    let is_private = json_value::read(
        json_value::read(json_value::read(payload, "base")?, "repo")?,
        "private",
    )?;
    let wanted_visibility = WantedVisibility::from_setting(settings.get("visibility"));
    let status = Status::from_check(wanted_visibility.accepts(json_value::truthy(is_private)));
    let mut input = Map::new();
    if let Some(private_flag) = is_private {
        input.insert("private".to_string(), private_flag.clone());
    }
    Ok(ProcessedOption {
        input: Value::Object(input),
        result: json!({"status": status.as_str()}),
    })
}
