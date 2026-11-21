use crate::context::Context;
use crate::github::client::{GithubApp, GithubAppSettings};
use crate::github::throttle::RequestThrottle;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const REPLAY_TEST_SIGNING_KEY: &str = include_str!("../../../../replay/test-key.pem");

pub fn context_for(event_name: &str, payload: Value) -> Context {
    let settings = GithubAppSettings {
        base_url: "http://127.0.0.1:9".to_string(),
        app_id: "1".to_string(),
        private_key_pem: REPLAY_TEST_SIGNING_KEY.to_string(),
        retry_base_delay: Duration::from_millis(1),
        throttle: RequestThrottle::like_probot(false),
    };
    let app = Arc::new(
        GithubApp::new(settings, None).unwrap_or_else(|error| panic!("test app: {error}")),
    );
    let mut context = Context::new(
        "event-id".to_string(),
        event_name.to_string(),
        payload,
        app.installation(Some(1)),
    );
    context.global_settings = Some(json!({}));
    context
}

pub struct PullRequest<'fields> {
    pub author: &'fields str,
    pub base_ref: &'fields str,
    pub repository_name: &'fields str,
    pub is_private: bool,
}

impl Default for PullRequest<'_> {
    fn default() -> Self {
        PullRequest {
            author: "creator",
            base_ref: "baseRef",
            repository_name: "repoName",
            is_private: false,
        }
    }
}

pub fn pull_request_context(pull_request: PullRequest<'_>) -> Context {
    context_for(
        "pull_request",
        json!({
            "action": "opened",
            "repository": {"name": pull_request.repository_name, "full_name": "fullRepoName", "owner": {"login": "owner"}},
            "sender": {"login": "initiator"},
            "pull_request": {
                "user": {"login": pull_request.author},
                "title": "title",
                "number": 1,
                "base": {"repo": {"full_name": "owner/test", "private": pull_request.is_private}, "ref": pull_request.base_ref, "sha": "sha2"},
                "head": {"ref": "test", "sha": "sha1"},
            },
        }),
    )
}

#[cfg(test)]
mod filter_base_tests {
    use crate::filters::{and_operator::And, author::Author, is_event_supported, payload::Payload};
    use crate::validators::validate_settings;
    use serde_json::json;

    #[test]
    fn pull_request_filters_support_pull_request_and_review_events_only() {
        assert!(is_event_supported(&Author, "pull_request.opened"));
        assert!(is_event_supported(&Author, "pull_request_review.submitted"));
        assert!(!is_event_supported(&Author, "issues.opened"));
        assert!(is_event_supported(&Payload, "issue_comment.created"));
        assert!(is_event_supported(&And, "schedule.repository"));
    }

    #[test]
    fn mismatching_setting_type_names_the_expected_type() {
        let supported = json!({"first": "string", "second": "number", "third": "array"});
        let error = validate_settings(
            "filter/test",
            &supported,
            &json!({"first": "Test", "second": "32", "third": [2, 3]}),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.message,
            "filter/test: second is expected to be of type: number"
        );
        assert!(
            validate_settings(
                "filter/test",
                &supported,
                &json!({"first": "Test", "second": 32}),
                &[]
            )
            .is_ok()
        );
    }

    #[test]
    fn unknown_setting_is_not_supported() {
        let error = validate_settings(
            "filter/test",
            &json!({"first": "string"}),
            &json!({"first": "Test", "second": 32}),
            &[],
        )
        .unwrap_err();
        assert_eq!(error.message, "filter/test: second option is not supported");
    }

    #[test]
    fn nested_unknown_setting_names_its_path() {
        let supported = json!({"first": "string", "second": {"third": "number"}});
        let error = validate_settings(
            "filter/test",
            &supported,
            &json!({"first": "Test", "second": {"test": 32}}),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            error.message,
            "filter/test: second.test option is not supported"
        );
        assert!(
            validate_settings(
                "filter/test",
                &supported,
                &json!({"first": "Test", "second": {"third": 32}}),
                &[]
            )
            .is_ok()
        );
    }

    #[test]
    fn setting_may_allow_several_types() {
        let supported = json!({"first": ["string", "array"]});
        assert!(
            validate_settings("filter/test", &supported, &json!({"first": "Test"}), &[]).is_ok()
        );
        assert!(validate_settings("filter/test", &supported, &json!({"first": [32]}), &[]).is_ok());
    }
}
