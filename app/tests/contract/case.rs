use anyhow::{Context as _, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Stub {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: Map<String, Value>,
    #[serde(default = "default_stub_status")]
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Value,
}

fn default_stub_status() -> u16 {
    200
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Expected {
    pub status: u16,
    pub calls: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaseDefinition {
    pub name: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub event: String,
    pub payload: Value,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub github: Vec<Stub>,
    #[serde(default)]
    pub expected: Option<Expected>,
    #[serde(default)]
    pub unordered: bool,
    #[serde(default)]
    pub known_diff: Option<String>,
    #[serde(default)]
    pub quiet_ms: Option<u64>,
}

impl CaseDefinition {
    pub fn repository_full_name(&self) -> String {
        self.payload
            .pointer("/repository/full_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    }

    #[must_use]
    pub fn delivery_id(&self) -> String {
        format!("replay-{}", self.name)
    }
}

pub struct Case {
    pub definition: CaseDefinition,
}

impl Case {
    pub fn load(file_path: &Path) -> Result<Case> {
        let text = std::fs::read_to_string(file_path)
            .with_context(|| format!("reading {}", file_path.display()))?;
        let definition: CaseDefinition = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", file_path.display()))?;
        let file_stem = file_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if definition.name != file_stem {
            bail!(
                "case name {:?} does not match file name {}",
                definition.name,
                file_path.display()
            );
        }
        Ok(Case { definition })
    }
}
