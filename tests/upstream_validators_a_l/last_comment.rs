use super::mock_context::mock_context;
use serde_json::{Value, json};

fn comment(body: &Value, commenter: Option<&Value>) -> Value {
    json!({"body": body, "user": {"login": commenter.cloned().unwrap_or_else(|| json!("creator"))}})
}

async fn validate_comments(
    comments: &[&str],
    commenters: Option<&[&str]>,
    event_name: &str,
    settings: &Value,
) -> Value {
    let comment_bodies = json!(comments);
    let commenter_logins = commenters.map(|logins| json!(logins));
    let listed_comments: Vec<Value> = comments
        .iter()
        .enumerate()
        .map(|(index, body)| {
            let commenter = commenters.map(|logins| json!(logins[index]));
            comment(&json!(body), commenter.as_ref())
        })
        .collect();
    mock_context(&json!({
        "eventName": event_name,
        "issueComment": comment(&comment_bodies, commenter_logins.as_ref()),
        "comments": listed_comments,
    }))
    .await
    .process_validate("lastComment", settings)
    .await
}

async fn status_of_pull_request_comments(
    comments: &[&str],
    commenters: Option<&[&str]>,
    settings: &Value,
) -> Value {
    validate_comments(comments, commenters, "pull_request", settings).await["status"].clone()
}

#[tokio::test]
async fn validate_returns_correctly() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}});
    assert_eq!(
        status_of_pull_request_comments(&["exclude this"], None, &settings).await,
        "fail"
    );
    assert_eq!(
        status_of_pull_request_comments(&["a", "b"], None, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn validate_applies_user_exclusions_correctly() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}, "comment_author": {"none_of": ["exclude_author"]}});
    assert_eq!(
        status_of_pull_request_comments(&["exclude this"], Some(&["exclude_author"]), &settings)
            .await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(&["exclude this"], Some(&["some[bot]"]), &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(&["a", "b"], Some(&["user_a", "user_b"]), &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(
            &["a", "exclude this"],
            Some(&["user_a", "user_b"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn validate_applies_bot_exclusions_correctly() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "a bot comment"}, "comment_author": {"no_bots": true}});
    assert_eq!(
        status_of_pull_request_comments(&["a bot comment"], Some(&["some[bot]"]), &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(&["a", "b"], Some(&["some[bot]", "user_b"]), &settings)
            .await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(
            &["a", "a bot comment"],
            Some(&["some[bot]", "user_b"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn validate_applies_bot_exclusions_correctly_when_no_bots_is_false() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "a bot comment"}, "comment_author": {"none_of": ["angry[bot]"], "no_bots": false}});
    assert_eq!(
        status_of_pull_request_comments(&["a bot comment"], Some(&["some[bot]"]), &settings).await,
        "fail"
    );
    assert_eq!(
        status_of_pull_request_comments(&["a bot comment"], Some(&["angry[bot]"]), &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(&["a", "b"], Some(&["user_a", "user_b"]), &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(
            &["a", "a bot comment"],
            Some(&["user_a", "user_b"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn validate_applies_at_author_exclusions_correctly() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}, "comment_author": {"none_of": ["@author"]}});
    assert_eq!(
        status_of_pull_request_comments(&["exclude this"], Some(&["creator"]), &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(
            &["a", "exclude this"],
            Some(&["creator", "user_b"]),
            &settings
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn validate_applies_at_author_inclusions_correctly() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}, "comment_author": {"one_of": ["@author"]}});
    assert_eq!(
        status_of_pull_request_comments(&["exclude this"], Some(&["creator"]), &settings).await,
        "fail"
    );
    assert_eq!(
        status_of_pull_request_comments(
            &["a", "exclude this"],
            Some(&["creator", "user_b"]),
            &settings
        )
        .await,
        "pass"
    );

    let settings_after_comment_author_was_consumed =
        json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}});
    assert_eq!(
        status_of_pull_request_comments(
            &["a", "exclude this"],
            Some(&["creator", "user_b"]),
            &settings_after_comment_author_was_consumed
        )
        .await,
        "fail"
    );
}

#[tokio::test]
async fn fail_gracefully_if_invalid_regex() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "@#$@#$@#$"}});
    assert_eq!(
        status_of_pull_request_comments(&["WIP"], None, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn description_is_correct() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "Work in Progress"}});
    let validation =
        validate_comments(&["Work in Progress"], None, "pull_request", &settings).await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "lastComment does not exclude \"Work in Progress\""
    );

    let validation =
        validate_comments(&["Just lastComment"], None, "pull_request", &settings).await;
    assert_eq!(
        validation["validations"][0]["description"],
        "lastComment must exclude 'Work in Progress'"
    );
}

#[tokio::test]
async fn mergeable_is_true_if_must_include_is_the_last_comment() {
    let settings = json!({"do": "lastComment", "must_include": {"regex": "xyz"}});
    assert_eq!(
        status_of_pull_request_comments(&["abc", "experimental", "xyz"], None, &settings).await,
        "pass"
    );
    assert_eq!(
        status_of_pull_request_comments(&["Some lastComment", "xyz", "456"], None, &settings).await,
        "fail"
    );
}

#[tokio::test]
async fn mergeable_is_false_if_must_exclude_is_one_of_the_last_comment() {
    let settings = json!({"do": "lastComment", "must_exclude": {"regex": "xyz"}});
    assert_eq!(
        status_of_pull_request_comments(&["abc", "experimental", "xyz"], None, &settings).await,
        "fail"
    );
    assert_eq!(
        status_of_pull_request_comments(&["Some lastComment", "xyz", "456"], None, &settings).await,
        "pass"
    );
}

#[tokio::test]
async fn mergeable_is_true_if_single_comment_got_edited_disregarding_whether_its_the_last_comment()
{
    let settings = json!({"do": "lastComment", "must_include": {"regex": "xyz"}, "must_exclude": {"regex": "ignore"}});
    let validation = validate_comments(
        &["abc", "experimental", "xyz"],
        None,
        "issue_comment",
        &settings,
    )
    .await;
    assert_eq!(validation["status"], "pass");
    let validation =
        validate_comments(&["abc", "xyz", "456"], None, "issue_comment", &settings).await;
    assert_eq!(validation["status"], "pass");
}

#[tokio::test]
async fn complex_logic_test() {
    let settings = json!({
        "do": "lastComment",
        "comment_author": {"none_of": ["e1"], "no_bots": false},
        "or": [
            {"and": [
                {"must_include": {"regex": "release note: yes", "message": "Please include release note: yes"}},
                {"must_include": {"regex": "lang\\/core|lang\\/c\\+\\+|lang\\/c#", "message": "Please include a language comment"}},
            ]},
            {"must_include": {"regex": "release note: no", "message": "Please include release note: no"}},
        ],
    });
    let validation = validate_comments(
        &["experimental", "xyz", "release note: no"],
        Some(&["u1", "u1", "u1"]),
        "pull_request",
        &settings,
    )
    .await;
    assert_eq!(validation["status"], "pass");

    let validation = validate_comments(
        &["123", "456", "release note: yes"],
        Some(&["u1", "u1", "u1"]),
        "pull_request",
        &settings,
    )
    .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "((Please include a language comment)  ***OR***  Please include release note: no)"
    );

    let validation = validate_comments(
        &["456", "lang/core"],
        Some(&["u1", "u1"]),
        "pull_request",
        &settings,
    )
    .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "((Please include release note: yes)  ***OR***  Please include release note: no)"
    );

    let validation = validate_comments(
        &["lang/core", "release note: yes"],
        Some(&["e1", "e1"]),
        "pull_request",
        &settings,
    )
    .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "((Please include release note: yes  ***AND***  Please include a language comment)  ***OR***  Please include release note: no)"
    );

    let mut settings_after_comment_author_was_consumed = settings.clone();
    settings_after_comment_author_was_consumed
        .as_object_mut()
        .expect("settings object")
        .shift_remove("comment_author");
    let validation = validate_comments(
        &["lang/core", "release note: yes"],
        Some(&["e1", "e1"]),
        "pull_request",
        &settings_after_comment_author_was_consumed,
    )
    .await;
    assert_eq!(validation["status"], "fail");
    assert_eq!(
        validation["validations"][0]["description"],
        "((Please include a language comment)  ***OR***  Please include release note: no)"
    );
}
