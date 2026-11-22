mod loader;
mod mark;
mod types;
mod value;

use crate::error::ErrorKind;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dialect {
    Lenient,
    Strict,
}

#[derive(Debug, Clone)]
pub struct YamlError {
    pub kind: ErrorKind,
    pub message: String,
}

impl YamlError {
    fn syntax(message: String) -> Self {
        Self {
            kind: ErrorKind::Yaml,
            message,
        }
    }

    #[must_use]
    pub fn is_syntax_error(&self) -> bool {
        self.kind == ErrorKind::Yaml
    }
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for YamlError {}

pub fn parse(text: &str) -> Result<Option<Value>, YamlError> {
    load_single_document(text, Dialect::Lenient)
}

pub fn parse_strict(text: &str) -> Result<Option<Value>, YamlError> {
    load_single_document(text, Dialect::Strict)
}

const LOADER_STACK_BYTES: usize = 64 * 1024 * 1024;

fn load_single_document(text: &str, dialect: Dialect) -> Result<Option<Value>, YamlError> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("yaml-loader".to_string())
            .stack_size(LOADER_STACK_BYTES)
            .spawn_scoped(scope, || load_on_current_thread(text, dialect))
            .map_err(|error| YamlError {
                kind: ErrorKind::General,
                message: error.to_string(),
            })?
            .join()
            .map_err(|_| YamlError {
                kind: ErrorKind::OutOfRange,
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
        return Err(YamlError::syntax(
            "expected a single document in the stream, but found more".to_string(),
        ));
    }
    value::to_json(&document, &heap, dialect).map(Some)
}
