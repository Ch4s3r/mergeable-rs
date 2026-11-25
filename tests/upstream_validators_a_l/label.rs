use super::mock_context::mock_context;
use serde_json::{Value, json};

async fn validate_labels(labels: &[&str], settings: &Value) -> Value {
    let label_objects: Vec<Value> = labels.iter().map(|label| json!({"name": label})).collect();
    mock_context(&json!({"labels": label_objects}))
        .await
        .process_validate("label", settings)
        .await
}

#[tokio::test]
async fn validate_returns_correctly() {
    let settings = json!({"do": "label", "must_exclude": {"regex": "wip"}});
    assert_eq!(validate_labels(&["wip"], &settings).await["status"], "fail");
    assert_eq!(
        validate_labels(&["a", "b"], &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn fail_gracefully_if_invalid_regex() {
    let settings = json!({"do": "label", "must_exclude": {"regex": "@#$@#$@#$"}});
    assert_eq!(validate_labels(&["WIP"], &settings).await["status"], "pass");
}

#[tokio::test]
async fn mergeable_is_false_if_regex_found_or_true_if_not_when_there_is_only_one_label() {
    let settings = json!({"do": "label", "must_include": {"regex": "Some Label"}});
    assert_eq!(
        validate_labels(&["work in progress"], &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_labels(&["Some Label"], &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn mergeable_is_false_if_regex_found_or_true_if_not_when_there_are_multiple_labels() {
    let settings = json!({"do": "label", "must_include": {"regex": "456"}});
    assert_eq!(
        validate_labels(&["abc", "experimental", "xyz"], &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_labels(&["Some Label", "123", "456"], &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn description_is_correct() {
    let settings = json!({"do": "label", "must_exclude": {"regex": "Work in Progress"}});
    let validation = validate_labels(&["Work in Progress"], &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "label does not exclude \"Work in Progress\""
    );

    let validation = validate_labels(&["Just Label"], &settings).await;
    assert_eq!(
        validation["validations"][0]["description"],
        "label must exclude 'Work in Progress'"
    );
}

#[tokio::test]
async fn mergeable_is_true_if_must_include_is_one_of_the_label() {
    let settings = json!({"do": "label", "must_include": {"regex": "abc"}});
    assert_eq!(
        validate_labels(&["abc", "experimental", "xyz"], &settings).await["status"],
        "pass"
    );
    assert_eq!(
        validate_labels(&["Some Label", "123", "456"], &settings).await["status"],
        "fail"
    );
}

#[tokio::test]
async fn mergeable_is_false_if_must_exclude_is_one_of_the_label() {
    let settings = json!({"do": "label", "must_exclude": {"regex": "xyz"}});
    assert_eq!(
        validate_labels(&["abc", "experimental", "xyz"], &settings).await["status"],
        "fail"
    );
    assert_eq!(
        validate_labels(&["Some Label", "123", "456"], &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn that_it_validates_ends_with_correctly() {
    let settings = json!({"do": "label", "ends_with": {"match": "test"}});
    let validation = validate_labels(&["Some Label"], &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "label must end with \"test\""
    );
    assert_eq!(
        validate_labels(&["Label test"], &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn complex_logic_test() {
    let settings = json!({"do": "label", "or": [
        {"and": [
            {"must_include": {"regex": "release note: yes", "message": "Please include release note: yes"}},
            {"must_include": {"regex": "lang\\/core|lang\\/c\\+\\+|lang\\/c#", "message": "Please include a language label"}},
        ]},
        {"must_include": {"regex": "release note: no", "message": "Please include release note: no"}},
    ]});
    assert_eq!(
        validate_labels(&["release note: no", "experimental", "xyz"], &settings).await["status"],
        "pass"
    );

    let validation = validate_labels(&["release note: yes", "123", "456"], &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "((Please include a language label)  ***OR***  Please include release note: no)"
    );

    let validation = validate_labels(&["lang/core", "456"], &settings).await;
    assert_eq!(
        validation["validations"][0]["description"],
        "((Please include release note: yes)  ***OR***  Please include release note: no)"
    );

    assert_eq!(
        validate_labels(&["release note: yes", "lang/core", "456"], &settings).await["status"],
        "pass"
    );
}
