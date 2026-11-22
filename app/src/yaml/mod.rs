mod loader;
mod mark;
mod types;
mod value;

use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dialect {
    JsYaml3,
    JsYaml4,
}

#[derive(Debug, Clone)]
pub struct YamlError {
    pub name: &'static str,
    pub message: String,
}

impl YamlError {
    fn yaml_exception(message: String) -> Self {
        Self {
            name: "YAMLException",
            message,
        }
    }

    #[must_use]
    pub fn is_yaml_exception(&self) -> bool {
        self.name == "YAMLException"
    }
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

pub fn parse(text: &str) -> Result<Option<Value>, YamlError> {
    load_single_document(text, Dialect::JsYaml3)
}

pub fn parse_like_js_yaml_4(text: &str) -> Result<Option<Value>, YamlError> {
    load_single_document(text, Dialect::JsYaml4)
}

const LOADER_STACK_BYTES: usize = 64 * 1024 * 1024;

fn load_single_document(text: &str, dialect: Dialect) -> Result<Option<Value>, YamlError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("yaml-loader".to_string())
            .stack_size(LOADER_STACK_BYTES)
            .spawn_scoped(scope, || load_on_current_thread(text, dialect))
            .map_err(|error| YamlError {
                name: "Error",
                message: error.to_string(),
            })?
            .join()
            .map_err(|_| YamlError {
                name: "RangeError",
                message: "Maximum call stack size exceeded".to_string(),
            })?
    })
}

fn load_on_current_thread(text: &str, dialect: Dialect) -> Result<Option<Value>, YamlError> {
    let (documents, heap) = loader::load_documents(text, dialect)?;
    let mut documents = documents.into_iter();
    let Some(document) = documents.next() else {
        return Ok(None);
    };
    if documents.next().is_some() {
        return Err(YamlError::yaml_exception(
            "expected a single document in the stream, but found more".to_string(),
        ));
    }
    value::to_json(&document, &heap, dialect).map(Some)
}
