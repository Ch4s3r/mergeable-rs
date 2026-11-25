use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
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
    pub file_path: PathBuf,
    pub document: Value,
    pub definition: CaseDefinition,
}

impl Case {
    pub fn load(file_path: &Path) -> Result<Case> {
        let text = std::fs::read_to_string(file_path)
            .with_context(|| format!("reading {}", file_path.display()))?;
        let document: Value = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", file_path.display()))?;
        let definition: CaseDefinition = serde_json::from_value(document.clone())
            .with_context(|| format!("reading case fields of {}", file_path.display()))?;
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
        Ok(Case {
            file_path: file_path.to_path_buf(),
            document,
            definition,
        })
    }

    pub fn save_expected(&mut self, expected: &Expected, unordered: bool) -> Result<()> {
        let Value::Object(fields) = &mut self.document else {
            bail!("{} is not a JSON object", self.file_path.display());
        };
        fields.insert("expected".to_owned(), serde_json::to_value(expected)?);
        fields.insert("unordered".to_owned(), Value::Bool(unordered));
        let mut text = serde_json::to_string_pretty(&self.document)?;
        text.push('\n');
        std::fs::write(&self.file_path, text)
            .with_context(|| format!("writing {}", self.file_path.display()))?;
        self.definition.expected = Some(expected.clone());
        self.definition.unordered = unordered;
        Ok(())
    }
}

pub fn load_cases(cases_directory: &Path, selected_names: &[String]) -> Result<Vec<Case>> {
    let mut case_files: Vec<PathBuf> = std::fs::read_dir(cases_directory)
        .with_context(|| format!("listing {}", cases_directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    case_files.sort();
    let cases = case_files
        .iter()
        .map(|path| Case::load(path))
        .collect::<Result<Vec<Case>>>()?;
    if selected_names.is_empty() {
        return Ok(cases);
    }
    for selected_name in selected_names {
        if !cases
            .iter()
            .any(|case| &case.definition.name == selected_name)
        {
            bail!(
                "no case named {selected_name} in {}",
                cases_directory.display()
            );
        }
    }
    Ok(cases
        .into_iter()
        .filter(|case| selected_names.contains(&case.definition.name))
        .collect())
}
