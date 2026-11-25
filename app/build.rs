use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn test_function_name(case_name: &str) -> String {
    let mut name: String = case_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if name
        .chars()
        .next()
        .is_none_or(|first| first.is_ascii_digit())
    {
        name.insert_str(0, "case_");
    }
    name
}

fn case_names(cases_directory: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut names = Vec::new();
    if !cases_directory.is_dir() {
        return Ok(names);
    }
    for entry in std::fs::read_dir(cases_directory)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
        {
            names.push(stem.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

fn generated_tests(case_names: &[String]) -> Result<String, Box<dyn Error>> {
    let mut used_function_names = BTreeSet::new();
    let mut source = String::new();
    for case_name in case_names {
        let mut function_name = test_function_name(case_name);
        while !used_function_names.insert(function_name.clone()) {
            function_name.push('_');
        }
        writeln!(
            source,
            "#[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]\nasync fn {function_name}() {{\n    run_contract_case({case_name:?}).await;\n}}\n"
        )?;
    }
    Ok(source)
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_directory = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let cases_directory = manifest_directory.join("../tests/cases");
    println!("cargo:rerun-if-changed={}", cases_directory.display());
    let source = generated_tests(&case_names(&cases_directory)?)?;
    let output_path = PathBuf::from(std::env::var("OUT_DIR")?).join("contract_cases.rs");
    std::fs::write(output_path, source)?;
    Ok(())
}
