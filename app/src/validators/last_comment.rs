use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::event_aware;
use crate::github::api;
use crate::js;
use crate::registry::Registry;
use crate::validators::options_processor::list_processor;
use crate::validators::{self, Validator};
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use std::borrow::Cow;

pub struct LastComment;

pub struct CommentAuthorFilter {
    pub exclude_bots: bool,
    pub author_option: Value,
}

fn is_bot_comment(comment: &Value) -> JsResult<bool> {
    let user = js::read(Some(comment), "user")?;
    match js::read(user, "login")? {
        Some(Value::String(login)) => Ok(login.to_lowercase().ends_with("[bot]")),
        None => Err(JsError::reading_property_of_undefined("toLowerCase")),
        Some(Value::Null) => Err(JsError::type_error(
            "Cannot read properties of null (reading 'toLowerCase')",
        )),
        Some(_) => Err(JsError::type_error(
            "c.user.login.toLowerCase is not a function",
        )),
    }
}

static EMPTY_BODY: Value = Value::String(String::new());

pub fn without_bot_comments(comments: &[Value], exclude_bots: bool) -> JsResult<Vec<&Value>> {
    let mut kept = Vec::new();
    for comment in comments {
        if !exclude_bots || !is_bot_comment(comment)? {
            kept.push(comment);
        }
    }
    Ok(kept)
}

pub fn last_comment_body<'comment>(comments: &[&'comment Value]) -> JsResult<&'comment Value> {
    match comments.last() {
        None => Ok(&EMPTY_BODY),
        Some(comment) => Ok(js::read(Some(comment), "body")?.unwrap_or(&js::NULL)),
    }
}

async fn comment_author_filter(
    context: &Context,
    settings: &Value,
) -> JsResult<CommentAuthorFilter> {
    let mut exclude_bots = true;
    let mut author_option = Map::new();
    if let Some(do_value) = settings.get("do") {
        author_option.insert("do".to_string(), do_value.clone());
    }
    if let Some(comment_author) = settings
        .get("comment_author")
        .filter(|value| js::is_truthy(value))
    {
        if let Some(one_of) =
            js::read(Some(comment_author), "one_of")?.filter(|value| js::is_truthy(value))
        {
            let candidates = list_processor::process(one_of, context).await?;
            author_option.insert("one_of".to_string(), Value::Array(candidates));
        }
        if let Some(none_of) =
            js::read(Some(comment_author), "none_of")?.filter(|value| js::is_truthy(value))
        {
            let candidates = list_processor::process(none_of, context).await?;
            author_option.insert("none_of".to_string(), Value::Array(candidates));
        }
        if comment_author.get("no_bots") == Some(&Value::Bool(false)) {
            exclude_bots = false;
        }
    }
    Ok(CommentAuthorFilter {
        exclude_bots,
        author_option: Value::Object(author_option),
    })
}

async fn filter_by_comment_author<'comment>(
    comments: &'comment [Value],
    filter: &CommentAuthorFilter,
) -> JsResult<Vec<&'comment Value>> {
    let candidates = without_bot_comments(comments, filter.exclude_bots)?;
    let mut kept = Vec::new();
    for comment in candidates {
        let login = js::read(js::read(Some(comment), "user")?, "login")?.unwrap_or(&Value::Null);
        let result = validators::process_options(&filter.author_option, login, None).await?;
        if result.get("status").and_then(Value::as_str) != Some("fail") {
            kept.push(comment);
        }
    }
    Ok(kept)
}

pub async fn evaluate(
    comments: &[Value],
    filter: &CommentAuthorFilter,
    remaining_settings: &Value,
) -> JsResult<Value> {
    let comments = filter_by_comment_author(comments, filter).await?;
    validators::process_options(remaining_settings, last_comment_body(&comments)?, None).await
}

#[async_trait]
impl Validator for LastComment {
    fn name(&self) -> &'static str {
        "lastComment"
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &[
            "pull_request.*",
            "pull_request_review.*",
            "issues.*",
            "issue_comment.created",
            "issue_comment.edited",
        ]
    }

    fn supported_settings(&self) -> Option<Value> {
        Some(json!({
            "must_include": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "must_exclude": {"regex": ["string", "array"], "regex_flag": "string", "message": "string"},
            "comment_author": {"one_of": "array", "none_of": "array", "no_bots": "boolean"},
        }))
    }

    async fn validate(
        &self,
        context: &Context,
        settings: &Value,
        _registry: &Registry,
    ) -> JsResult<Value> {
        let filter = comment_author_filter(context, settings).await?;
        let mut remaining_settings = Cow::Borrowed(settings);
        if settings.get("comment_author").is_some()
            && let Value::Object(map) = remaining_settings.to_mut()
        {
            map.shift_remove("comment_author");
        }

        let issue_number =
            js::read(event_aware::get_payload(context), "number")?.unwrap_or(&Value::Null);
        let listed_comments;
        let comments = if context.event_name == "issue_comment" {
            let comment = context
                .payload
                .get("comment")
                .ok_or_else(|| JsError::reading_property_of_undefined("user"))?;
            std::slice::from_ref(comment)
        } else {
            listed_comments = api::list_comments(context, issue_number).await?;
            listed_comments.as_slice()
        };

        evaluate(comments, &filter, &remaining_settings).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(body: &str, login: &str) -> Value {
        json!({"body": body, "user": {"login": login}})
    }

    #[test]
    fn bot_comments_are_dropped_by_default() {
        let comments = vec![
            comment("a bot comment", "Some[BOT]"),
            comment("human", "user_b"),
        ];
        let kept = without_bot_comments(&comments, true).unwrap();
        assert_eq!(last_comment_body(&kept).unwrap(), &json!("human"));
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn bot_comments_are_kept_when_no_bots_is_false() {
        let comments = vec![
            comment("human", "user_b"),
            comment("a bot comment", "some[bot]"),
        ];
        let kept = without_bot_comments(&comments, false).unwrap();
        assert_eq!(last_comment_body(&kept).unwrap(), &json!("a bot comment"));
    }

    #[test]
    fn no_remaining_comment_validates_an_empty_body() {
        assert_eq!(last_comment_body(&[]).unwrap(), &json!(""));
    }

    #[test]
    fn comment_without_user_throws_like_node() {
        let error = without_bot_comments(&[json!({"body": "x"})], true).unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'login')"
        );
    }

    fn comments(bodies: &[&str], authors: &[&str]) -> Vec<Value> {
        bodies
            .iter()
            .enumerate()
            .map(|(index, body)| comment(body, authors.get(index).copied().unwrap_or("creator")))
            .collect()
    }

    fn author_filter(exclude_bots: bool, author_option: Value) -> CommentAuthorFilter {
        CommentAuthorFilter {
            exclude_bots,
            author_option,
        }
    }

    async fn result_of(
        bodies: &[&str],
        authors: &[&str],
        filter: CommentAuthorFilter,
        settings: Value,
    ) -> Value {
        evaluate(&comments(bodies, authors), &filter, &settings)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn must_exclude_checks_only_the_last_comment() {
        let settings = json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}});
        let filter = || author_filter(true, json!({"do": "lastComment"}));
        assert_eq!(
            result_of(&["exclude this"], &[], filter(), settings.clone()).await["status"],
            "fail"
        );
        assert_eq!(
            result_of(&["a", "b"], &[], filter(), settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn comments_of_excluded_authors_are_ignored() {
        let settings = json!({"do": "lastComment", "must_exclude": {"regex": "exclude this"}});
        let filter = || {
            author_filter(
                true,
                json!({"do": "lastComment", "none_of": ["exclude_author"]}),
            )
        };
        assert_eq!(
            result_of(
                &["exclude this"],
                &["exclude_author"],
                filter(),
                settings.clone()
            )
            .await["status"],
            "pass"
        );
        assert_eq!(
            result_of(
                &["exclude this"],
                &["some[bot]"],
                filter(),
                settings.clone()
            )
            .await["status"],
            "pass"
        );
        assert_eq!(
            result_of(
                &["a", "exclude this"],
                &["user_a", "user_b"],
                filter(),
                settings
            )
            .await["status"],
            "fail"
        );
    }

    #[tokio::test]
    async fn bots_count_when_no_bots_is_false_unless_explicitly_excluded() {
        let settings = json!({"do": "lastComment", "must_exclude": {"regex": "a bot comment"}});
        let filter = || {
            author_filter(
                false,
                json!({"do": "lastComment", "none_of": ["angry[bot]"]}),
            )
        };
        assert_eq!(
            result_of(
                &["a bot comment"],
                &["some[bot]"],
                filter(),
                settings.clone()
            )
            .await["status"],
            "fail"
        );
        assert_eq!(
            result_of(&["a bot comment"], &["angry[bot]"], filter(), settings).await["status"],
            "pass"
        );
    }

    #[tokio::test]
    async fn description_names_the_validator_and_pattern() {
        let settings = json!({"do": "lastComment", "must_exclude": {"regex": "Work in Progress"}});
        let filter = || author_filter(true, json!({"do": "lastComment"}));
        let failing = result_of(&["Work in Progress"], &[], filter(), settings.clone()).await;
        assert_eq!(
            failing["validations"][0]["description"],
            "lastComment does not exclude \"Work in Progress\""
        );
        let passing = result_of(&["Just lastComment"], &[], filter(), settings).await;
        assert_eq!(
            passing["validations"][0]["description"],
            "lastComment must exclude 'Work in Progress'"
        );
    }

    #[tokio::test]
    async fn nested_and_or_options_report_the_failing_branches() {
        let settings = json!({
            "do": "lastComment",
            "or": [
                {"and": [
                    {"must_include": {"regex": "release note: yes", "message": "Please include release note: yes"}},
                    {"must_include": {"regex": "lang\\/core|lang\\/c\\+\\+|lang\\/c#", "message": "Please include a language comment"}},
                ]},
                {"must_include": {"regex": "release note: no", "message": "Please include release note: no"}},
            ],
        });
        let filter = || author_filter(false, json!({"do": "lastComment", "none_of": ["e1"]}));
        let release_note_no = result_of(
            &["experimental", "xyz", "release note: no"],
            &["u1", "u1", "u1"],
            filter(),
            settings.clone(),
        )
        .await;
        assert_eq!(release_note_no["status"], "pass");
        let missing_language = result_of(
            &["123", "456", "release note: yes"],
            &["u1", "u1", "u1"],
            filter(),
            settings.clone(),
        )
        .await;
        assert_eq!(
            missing_language["validations"][0]["description"],
            "((Please include a language comment)  ***OR***  Please include release note: no)"
        );
        let excluded_author = result_of(
            &["lang/core", "release note: yes"],
            &["e1", "e1"],
            filter(),
            settings,
        )
        .await;
        assert_eq!(excluded_author["status"], "fail");
    }
}
