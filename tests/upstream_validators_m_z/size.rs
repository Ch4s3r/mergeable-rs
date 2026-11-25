use super::{MockedGithub, PullRequestFields, get_stub, pull_request_payload};
use axum::Router;
use axum::routing::get;
use mergeable::validators::size::Size;
use serde_json::{Value, json};

const TOO_BIG: &str = "Too big!";
const ADDITIONS_OK: &str = "PR size for additions is OK!";
const DELETIONS_OK: &str = "PR size for deletions is OK!";
const TOTAL_OK: &str = "PR size for total additions + deletions is OK!";

fn pull_request_files() -> Value {
    json!([
        {"filename": "thing.js", "status": "modified", "additions": 10, "deletions": 5, "changes": 15},
        {"filename": "another.js", "status": "added", "additions": 3, "deletions": 2, "changes": 5},
        {"filename": "removed_file_should_be_ignored.js", "status": "removed", "additions": 0, "deletions": 500, "changes": 500},
    ])
}

fn line_limits(additions: u64, deletions: u64, total: u64) -> Value {
    json!({
        "additions": {"count": additions, "message": TOO_BIG},
        "deletions": {"count": deletions, "message": TOO_BIG},
        "total": {"count": total, "message": TOO_BIG},
    })
}

fn total_limit_ignoring_comments(count: u64) -> Value {
    json!({"do": "size", "lines": {"total": {"count": count, "message": TOO_BIG}, "ignore_comments": true}})
}

async fn github_listing(files: Value, diff_url: Option<String>) -> MockedGithub {
    MockedGithub::start(
        vec![get_stub("/repos/owner/repo/pulls/1/files", files)],
        pull_request_payload(PullRequestFields {
            diff_url,
            ..PullRequestFields::default()
        }),
    )
    .await
}

async fn validate_size(files: Value, settings: &Value) -> Value {
    github_listing(files, None)
        .await
        .process_validate(&Size, settings)
        .await
}

async fn serve_diff(diff: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("diff server address");
    let port = listener.local_addr().expect("diff server port").port();
    let router = Router::new().route("/diff", get(move || async move { diff }));
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("diff server");
    });
    format!("http://127.0.0.1:{port}/diff")
}

async fn validate_size_of_diff(diff: &'static str, settings: &Value) -> Value {
    let diff_url = serve_diff(diff).await;
    github_listing(pull_request_files(), Some(diff_url))
        .await
        .process_validate(&Size, settings)
        .await
}

fn assert_validations(validation: &Value, expected: &[(&str, &str)]) {
    for (index, (status, description)) in expected.iter().enumerate() {
        assert_eq!(validation["validations"][index]["status"], *status);
        assert_eq!(
            validation["validations"][index]["description"],
            *description
        );
    }
}

#[tokio::test]
async fn max_and_total_together_are_an_error() {
    let settings = json!({"do": "size", "lines": {"max": {"count": 10, "message": TOO_BIG}, "total": {"count": 10, "message": TOO_BIG}}});
    let validation = validate_size(pull_request_files(), &settings).await;
    assert_eq!(validation["status"], "error");
    assert_validations(
        &validation,
        &[(
            "error",
            "Options max and total cannot be used together. Please choose one",
        )],
    );
}

#[tokio::test]
async fn max_stays_backwards_compatible_as_total() {
    let settings = json!({"do": "size", "lines": {"max": {"count": 10, "message": TOO_BIG}}});
    let validation = validate_size(pull_request_files(), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_validations(&validation, &[("fail", TOO_BIG)]);
}

#[tokio::test]
async fn missing_lines_configuration_is_an_error() {
    let validation = validate_size(pull_request_files(), &json!({"do": "size"})).await;
    assert_eq!(validation["status"], "error");
    assert_validations(
        &validation,
        &[(
            "error",
            "Failed to validate because the 'lines' or 'max / total', 'additions' or 'deletions' option is missing. Please check the documentation.",
        )],
    );
}

#[tokio::test]
async fn fails_when_size_of_changes_is_above_count() {
    let settings = json!({"do": "size", "lines": line_limits(10, 5, 10)});
    let validation = validate_size(pull_request_files(), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_validations(
        &validation,
        &[("fail", TOO_BIG), ("fail", TOO_BIG), ("fail", TOO_BIG)],
    );
}

#[tokio::test]
async fn passes_when_changes_equal_the_count() {
    let settings = json!({"do": "size", "lines": line_limits(13, 7, 20)});
    let validation = validate_size(pull_request_files(), &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_validations(
        &validation,
        &[
            ("pass", ADDITIONS_OK),
            ("pass", DELETIONS_OK),
            ("pass", TOTAL_OK),
        ],
    );
}

#[tokio::test]
async fn fails_partially_when_only_part_of_the_pull_request_is_within_count() {
    let settings = json!({"do": "size", "lines": line_limits(13, 5, 20)});
    let validation = validate_size(pull_request_files(), &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_validations(
        &validation,
        &[
            ("pass", ADDITIONS_OK),
            ("fail", TOO_BIG),
            ("pass", TOTAL_OK),
        ],
    );
}

#[tokio::test]
async fn ignores_specified_files() {
    let settings = json!({"do": "size", "lines": line_limits(10, 5, 15), "ignore": ["another.js"]});
    let validation = validate_size(pull_request_files(), &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_validations(
        &validation,
        &[
            ("pass", ADDITIONS_OK),
            ("pass", DELETIONS_OK),
            ("pass", TOTAL_OK),
        ],
    );
}

#[tokio::test]
async fn matches_hidden_files() {
    let files = json!([
        {"filename": ".test/thing.js", "status": "modified", "additions": 20, "deletions": 5, "changes": 15},
    ]);
    let settings = json!({"do": "size", "lines": {"additions": {"count": 10, "message": TOO_BIG}}});
    let validation = validate_size(files, &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_validations(&validation, &[("fail", TOO_BIG)]);
}

#[tokio::test]
async fn ignores_and_matches_glob_patterns() {
    let files = json!([
        {"filename": "not_too_big.js", "status": "modified", "additions": 10, "deletions": 0, "changes": 10},
        {"filename": "nested/one/big_file_1.js", "status": "added", "additions": 30, "deletions": 0, "changes": 30},
        {"filename": "nested/one/big_file_2.js", "status": "added", "additions": 30, "deletions": 0, "changes": 30},
        {"filename": "nested/two/three/big_file_3.js", "status": "modified", "additions": 30, "deletions": 0, "changes": 30},
    ]);
    let all_within_limits = [
        ("pass", ADDITIONS_OK),
        ("pass", DELETIONS_OK),
        ("pass", TOTAL_OK),
    ];
    let github = github_listing(files, None).await;

    let ignored_directories = json!({"do": "size", "lines": line_limits(10, 5, 15), "ignore": ["nested/one/*", "nested/two/**"]});
    let validation = github.process_validate(&Size, &ignored_directories).await;
    assert_eq!(validation["status"], "pass");
    assert_validations(&validation, &all_within_limits);

    let ignored_file_names =
        json!({"do": "size", "lines": line_limits(10, 5, 15), "ignore": ["**/big_file_*.js"]});
    let validation = github.process_validate(&Size, &ignored_file_names).await;
    assert_eq!(validation["status"], "pass");
    assert_validations(&validation, &all_within_limits);

    let matched_file_names =
        json!({"do": "size", "lines": line_limits(10, 5, 15), "ignore": [], "match": ["not_*.js"]});
    let validation = github.validate(&Size, &matched_file_names).await;
    assert_eq!(validation["status"], "pass");
    assert_validations(&validation, &all_within_limits);
}

#[tokio::test]
async fn handles_empty_ignore_list() {
    let too_small_limits = json!({"do": "size", "lines": line_limits(10, 5, 15), "ignore": []});
    let validation = validate_size(pull_request_files(), &too_small_limits).await;
    assert_eq!(validation["status"], "fail");
    assert_validations(
        &validation,
        &[("fail", TOO_BIG), ("fail", TOO_BIG), ("fail", TOO_BIG)],
    );

    let exact_limits = json!({"do": "size", "lines": line_limits(13, 7, 20), "ignore": []});
    let validation = validate_size(pull_request_files(), &exact_limits).await;
    assert_eq!(validation["status"], "pass");
    assert_validations(
        &validation,
        &[
            ("pass", ADDITIONS_OK),
            ("pass", DELETIONS_OK),
            ("pass", TOTAL_OK),
        ],
    );
}

#[tokio::test]
async fn both_single_line_comments_and_block_comments_are_ignored() {
    let settings = total_limit_ignoring_comments(1);
    let diff_with_code = "diff --git a/commentTestFiles.js b/commentTestFiles.js\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+// this is a comment\n+ // second comment\n+ non comment\n+\n+/*\n+    Comment block\n+ */\n";
    let validation = validate_size_of_diff(diff_with_code, &settings).await;
    assert_eq!(validation["status"], "fail");

    let diff_with_comments_only = "diff --git a/commentTestFiles.js b/commentTestFiles.js\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+// this is a comment\n+/*\n+    Comment block\n+ */\n";
    let validation = validate_size_of_diff(diff_with_comments_only, &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(validation["validations"][0]["description"], TOTAL_OK);
}

#[tokio::test]
async fn comments_in_unsupported_file_extensions_are_counted() {
    let diff = "diff --git a/commentTestFiles.notsupportedExtension b/commentTestFiles.notsupportedExtension\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.notsupportedExtension\n@@ -0,0 +1,5 @@\n+// this is a comment\n+/*\n+    Comment block\n+ */\n";
    let validation = validate_size_of_diff(diff, &total_limit_ignoring_comments(1)).await;
    assert_eq!(validation["status"], "fail");
}

#[tokio::test]
async fn js_file_extension_ignores_line_and_block_comments() {
    let settings = total_limit_ignoring_comments(1);
    let line_comments = "diff --git a/commentTestFiles.js b/commentTestFiles.js\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+// this is a comment\n+ // second comment\n";
    let validation = validate_size_of_diff(line_comments, &settings).await;
    assert_eq!(validation["status"], "pass");

    let block_comment = "diff --git a/commentTestFiles.js b/commentTestFiles.js\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+/*\n+    Comment block\n+ */\n";
    let validation = validate_size_of_diff(block_comment, &settings).await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn py_file_extension_ignores_hash_comments() {
    let settings = total_limit_ignoring_comments(0);
    let python_code_under_js_header = "diff --git a/commentTestFiles.py b/commentTestFiles.py\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.js\n@@ -0,0 +1,5 @@\n+# this is a comment\n+ # second comment\n+ non comment\n";
    let validation = validate_size_of_diff(python_code_under_js_header, &settings).await;
    assert_eq!(validation["status"], "fail");

    let python_comments = "diff --git a/commentTestFiles.py b/commentTestFiles.py\nnew file mode 100644\nindex 0000000..983b141\n--- /dev/null\n+++ b/commentTestFiles.py\n@@ -0,0 +1,5 @@\n+# this is a comment\n+ # second comment\n";
    let validation = validate_size_of_diff(python_comments, &settings).await;
    assert_eq!(validation["status"], "pass");
}
