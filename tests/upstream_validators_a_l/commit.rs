use super::mock_context::mock_context;
use serde_json::{Value, json};

fn now_milliseconds() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

fn commit_by(date: i64, message: &str, author_email: &str, committer_email: &str) -> Value {
    json!({"commit": {
        "author": {"date": date, "name": "Monalisa Octocat", "email": author_email},
        "committer": {"date": date, "name": "Valdis Ferdinand", "email": committer_email},
        "message": message,
    }})
}

fn commit_with_message(date: i64, message: &str) -> Value {
    json!({"commit": {"author": {"date": date}, "message": message}})
}

async fn validate_commits(commits: &[Value], settings: &Value) -> Value {
    mock_context(&json!({"commits": commits}))
        .await
        .process_validate("commit", settings)
        .await
}

#[tokio::test]
async fn validate_returns_correctly() {
    let settings = json!({"do": "commit", "message": {"regex": "feat:"}});
    let date = now_milliseconds();
    let mut commits = vec![
        commit_by(date, "fix: this", "support@github.com", "valdis@github.com"),
        commit_by(
            date + 1,
            "feat: that",
            "support1@github.com",
            "valdis1@github.com",
        ),
    ];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "fail"
    );

    commits[0]["commit"]["message"] = json!("feat: this");
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn oldest_only_sub_option() {
    let settings = json!({"do": "commit", "message": {"regex": "feat:", "oldest_only": true}});
    let date = now_milliseconds();
    let mut commits = vec![
        commit_with_message(date, "fix: this"),
        commit_with_message(date + 1, "fix: that"),
    ];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "fail"
    );

    commits[0]["commit"]["message"] = json!("feat: this");
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn newest_only_sub_option() {
    let settings = json!({"do": "commit", "message": {"regex": "feat:", "newest_only": true}});
    let date = now_milliseconds();
    let mut commits = vec![
        commit_with_message(date, "fix: that"),
        commit_with_message(date + 1, "fix: this"),
    ];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "fail"
    );

    commits[1]["commit"]["message"] = json!("feat: this");
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn skip_merge_sub_option() {
    let date = now_milliseconds();
    let commits = vec![
        commit_with_message(date, "feat: this"),
        commit_with_message(date + 1, "Merge branch"),
    ];
    let checking_merges =
        json!({"do": "commit", "message": {"regex": "feat:", "skip_merge": false}});
    assert_eq!(
        validate_commits(&commits, &checking_merges).await["status"],
        "fail"
    );

    let skipping_merges =
        json!({"do": "commit", "message": {"regex": "feat:", "skip_merge": true}});
    assert_eq!(
        validate_commits(&commits, &skipping_merges).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn single_commit_only_sub_option() {
    let settings =
        json!({"do": "commit", "message": {"regex": "feat:", "single_commit_only": true}});
    let date = now_milliseconds();
    let mut commits = vec![
        commit_with_message(date, "fix: this"),
        commit_with_message(date + 1, "test"),
    ];
    let validation = validate_commits(&commits, &settings).await;
    assert_eq!(validation["status"], "pass");
    assert_eq!(
        validation["validations"][0]["description"],
        "Since there are more than one commits, Skipping validation"
    );

    commits.pop();
    let validation = validate_commits(&commits, &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "Some or all of your commit messages doesn't meet the criteria"
    );
}

#[tokio::test]
async fn author_email_option_should_be_used() {
    let settings = json!({"do": "commit", "message": {"regex": "^[A-Za-z0-9._%+-]+@github.com$", "message_type": "author_email"}});
    let date = now_milliseconds();
    let commits = vec![
        commit_by(date, "fix: this", "support@github.com", "valdis@github.com"),
        commit_by(
            date + 1,
            "feat: that",
            "support1@github.com",
            "valdis1@github.com",
        ),
    ];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn author_email_option_should_match_the_regex() {
    let settings = json!({"do": "commit", "message": {"regex": "^[A-Za-z0-9._%+-]+@github.com$", "message_type": "author_email"}});
    let commits = vec![commit_by(
        now_milliseconds() + 1,
        "feat: that",
        "support1@github1.com",
        "valdis1@github1.com",
    )];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "fail"
    );
}

#[tokio::test]
async fn committer_email_option_should_be_used() {
    let settings = json!({"do": "commit", "message": {"regex": "^[A-Za-z0-9._%+-]+@github.com$", "message_type": "committer_email"}});
    let date = now_milliseconds();
    let commits = vec![
        commit_by(date, "fix: this", "support@github.com", "valdis@github.com"),
        commit_by(
            date + 1,
            "feat: that",
            "support1@github.com",
            "valdis1@github.com",
        ),
    ];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "pass"
    );
}

#[tokio::test]
async fn committer_email_option_should_match_the_regex() {
    let settings = json!({"do": "commit", "message": {"regex": "^[A-Za-z0-9._%+-]+@github.com$", "message_type": "committer_email"}});
    let commits = vec![commit_by(
        now_milliseconds() + 1,
        "feat: that",
        "support1@github.com",
        "valdis1@github1.com",
    )];
    assert_eq!(
        validate_commits(&commits, &settings).await["status"],
        "fail"
    );
}
