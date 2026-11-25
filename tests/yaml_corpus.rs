use mergeable::error::ErrorKind;
use mergeable::yaml::{self, YamlError};
use serde_json::{Map, Number, Value, json};

const YAML_CORPUS: &str = include_str!("fixtures/yaml.json");

fn numbers_to_fifteen_significant_digits(value: &Value) -> Value {
    match value {
        Value::Number(number) => number
            .as_f64()
            .and_then(|float| format!("{float:.14e}").parse::<f64>().ok())
            .and_then(Number::from_f64)
            .map_or(Value::Null, Value::Number),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(numbers_to_fifteen_significant_digits)
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, item)| (key.clone(), numbers_to_fifteen_significant_digits(item)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

fn outcome_of(parsed: Result<Option<Value>, YamlError>) -> Value {
    match parsed {
        Ok(Some(output)) => json!({"output": numbers_to_fifteen_significant_digits(&output)}),
        Ok(None) => json!({"empty": true}),
        Err(error) => json!({"error": error.message, "errorName": error.kind.name()}),
    }
}

fn expected_outcome(recorded: &Value) -> Value {
    if let Some(output) = recorded.get("output") {
        return json!({"output": numbers_to_fifteen_significant_digits(output)});
    }
    if recorded.get("empty").is_some() {
        return json!({"empty": true});
    }
    json!({"error": recorded["error"], "errorName": recorded["errorName"]})
}

fn mismatches_against_corpus(
    parse: fn(&str) -> Result<Option<Value>, YamlError>,
    recorded_outcome: fn(&Value) -> &Value,
) -> (usize, Vec<String>) {
    let corpus: Vec<Value> = serde_json::from_str(YAML_CORPUS).unwrap();
    let mismatches = corpus
        .iter()
        .filter_map(|entry| {
            let input = entry["input"].as_str().unwrap_or_default();
            let expected = expected_outcome(recorded_outcome(entry));
            let actual = outcome_of(parse(input));
            (actual != expected)
                .then(|| format!("input {input:?}\n  expected {expected}\n  actual   {actual}"))
        })
        .collect();
    (corpus.len(), mismatches)
}

#[test]
fn every_corpus_document_parses_leniently_as_recorded() {
    let (total, mismatches) = mismatches_against_corpus(yaml::parse, |entry| entry);
    assert!(
        mismatches.is_empty(),
        "{} of {total} corpus entries differ from the recorded lenient outcome:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn every_corpus_document_parses_strictly_as_recorded() {
    let (total, mismatches) =
        mismatches_against_corpus(yaml::parse_strict, |entry| &entry["strict"]);
    assert!(
        mismatches.is_empty(),
        "{} of {total} corpus entries differ from the recorded strict outcome:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn deeply_nested_documents_stop_with_a_range_error_instead_of_overflowing_the_stack() {
    let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(yaml::parse(&nested(990)).is_ok());
    let error = yaml::parse(&nested(5000)).unwrap_err();
    assert_eq!(
        (error.kind, error.message.as_str()),
        (ErrorKind::OutOfRange, "Maximum call stack size exceeded")
    );
    let block_mapping = (0..990)
        .map(|depth| " ".repeat(depth) + "k:\n")
        .collect::<String>();
    assert!(yaml::parse(&block_mapping).is_ok());
}

#[test]
fn invalid_pull_request_config_reports_the_bad_indentation_message() {
    let config = "version: 2\nmergeable:\n  - when: pull_request.*\n     validate: [\n";
    let error = yaml::parse(config).unwrap_err();
    assert_eq!(
        error.message,
        "bad indentation of a mapping entry at line 4, column 14:\n         validate: [\n                 ^"
    );
}
