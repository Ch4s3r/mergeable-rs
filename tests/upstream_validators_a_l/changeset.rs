use super::mock_context::mock_context;
use serde_json::{Value, json};

async fn changeset_status(files: Value, settings: &Value) -> Value {
    mock_context(&json!({"files": files}))
        .await
        .process_validate("changeset", settings)
        .await["status"]
        .clone()
}

#[tokio::test]
async fn validate_returns_correctly() {
    let settings = json!({"do": "changeset", "must_exclude": {"regex": "b.js"}});
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
            &settings
        )
        .await,
        "pass"
    );
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "b.js"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn supports_arrays() {
    let settings = json!({"do": "changeset", "must_include": {"regex": ["a.js", "b.js", "c.md"]}});
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
            &settings
        )
        .await,
        "pass"
    );
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "b.js"]),
            &settings
        )
        .await,
        "pass"
    );
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn fail_gracefully_if_invalid_regex() {
    let settings = json!({"do": "changeset", "must_include": {"regex": "@#$@#$@#$"}});
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn mergeable_is_true_if_must_include_is_one_of_the_label() {
    let settings = json!({"do": "changeset", "must_include": {"regex": "a.js"}});
    assert_eq!(
        changeset_status(
            json!([
                "package-lock.json",
                "yarn.lock",
                "package.json",
                "a.js",
                "a.jsx"
            ]),
            &settings
        )
        .await,
        "pass"
    );
}

#[tokio::test]
async fn mergeable_is_false_if_must_exclude_is_one_of_the_label() {
    let settings = json!({"do": "changeset", "must_exclude": {"regex": "yarn.lock"}});
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn that_it_validates_ends_with_correctly() {
    let settings = json!({"do": "changeset", "ends_with": {"match": "lock"}});
    assert_eq!(
        changeset_status(
            json!(["package-lock.json", "yarn.lock", "package.json", "a.js"]),
            &settings
        )
        .await,
        "pass"
    );
}

#[tokio::test]
async fn that_it_validates_must_include_with_all_true_correctly() {
    let settings =
        json!({"do": "changeset", "must_include": {"regex": "^\\w+\\.md$", "all": true}});
    assert_eq!(
        changeset_status(json!(["test.md", "another.md", "something.md"]), &settings).await,
        "pass"
    );
    assert_eq!(
        changeset_status(json!(["test.md", "not_md.js", "another.md"]), &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn that_it_validates_must_exclude_with_all_true_correctly() {
    let settings =
        json!({"do": "changeset", "must_exclude": {"regex": "^\\w+\\.md$", "all": true}});
    assert_eq!(
        changeset_status(json!(["test.js", "another.py", "something.txt"]), &settings).await,
        "pass"
    );
    assert_eq!(
        changeset_status(json!(["another.py", "test.md", "something.txt"]), &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn correct_files_are_considered_based_on_file_status_setting() {
    let settings = json!({
        "do": "changeset",
        "files": {"added": true, "modified": false},
        "must_include": {"regex": "added-file.js"},
        "must_exclude": {"regex": "(modified-file.py)|(deleted-file.ts)"},
    });
    let files = json!([
        {"filename": "added-file.js", "status": "added", "additions": 1, "changes": 0, "deletions": 0},
        {"filename": "modified-file.py", "status": "modified", "additions": 0, "changes": 1, "deletions": 0},
        {"filename": "deleted-file.ts", "status": "deleted", "additions": 0, "changes": 0, "deletions": 1},
    ]);
    assert_eq!(changeset_status(files, &settings).await, "pass");
}
