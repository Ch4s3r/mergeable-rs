use crate::error::Result;
use crate::event_aware::EventAttributes;
use crate::json_value;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
enum Annotation {
    Author,
    Action,
    Bot,
    Repository,
    Sender,
}

impl Annotation {
    const ALL: [Annotation; 5] = [
        Annotation::Author,
        Annotation::Action,
        Annotation::Bot,
        Annotation::Repository,
        Annotation::Sender,
    ];

    fn token(self) -> &'static str {
        match self {
            Annotation::Author => "@author",
            Annotation::Action => "@action",
            Annotation::Bot => "@bot",
            Annotation::Repository => "@repository",
            Annotation::Sender => "@sender",
        }
    }

    fn value(self, payload: Option<&Value>, event: &EventAttributes<'_>) -> Result<String> {
        Ok(match self {
            Annotation::Author => json_value::to_display_string(json_value::read(
                json_value::read(payload, "user")?,
                "login",
            )?),
            Annotation::Action => json_value::to_display_string(event.action),
            Annotation::Bot => bot_name(event.app_name),
            Annotation::Repository => display_or_empty(
                event
                    .repository
                    .and_then(|repository| json_value::get(repository, "full_name")),
            ),
            Annotation::Sender => display_or_empty(json_value::read(event.sender, "login")?),
        })
    }
}

#[must_use]
pub fn bot_name(app_name: Option<&str>) -> String {
    format!("{}[bot]", app_name.unwrap_or("Mergeable"))
}

fn display_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        present => json_value::to_display_string(present),
    }
}

fn replace_first_unescaped(text: &mut String, annotation: &str, replacement: &str) {
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find(annotation) {
        let position = search_from + offset;
        if position == 0 || !text[..position].ends_with('\\') {
            text.replace_range(position..position + annotation.len(), replacement);
            return;
        }
        search_from = position + 1;
    }
}

pub fn search_and_replace_special_annotations(
    template: &str,
    payload: Option<&Value>,
    event: &EventAttributes<'_>,
) -> Result<String> {
    let mut result = template.to_string();
    for annotation in Annotation::ALL {
        let replacement = annotation.value(payload, event)?;
        let annotation = annotation.token();
        replace_first_unescaped(&mut result, annotation, &replacement);
        if let Some(backslash_position) = result.find(&format!("\\{annotation}")) {
            result.remove(backslash_position);
        }
        if result.starts_with(annotation) {
            result.replace_range(..annotation.len(), &replacement);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> Value {
        json!({"action": "opened", "repository": {"full_name": "acme/app"}, "sender": {"login": "octocat"}})
    }

    #[test]
    fn first_unescaped_annotation_is_replaced_and_escape_is_removed() {
        let payload = json!({"user": {"login": "alice"}});
        let rendered = search_and_replace_special_annotations(
            "hi @author and \\@author by @sender in @repository",
            Some(&payload),
            &EventAttributes::from_payload(&event()),
        )
        .unwrap();
        assert_eq!(rendered, "hi alice and @author by octocat in acme/app");
    }

    #[test]
    fn escaped_annotation_at_start_is_replaced_after_unescaping() {
        let payload = json!({"user": {"login": "alice"}});
        let rendered = search_and_replace_special_annotations(
            "\\@author",
            Some(&payload),
            &EventAttributes::from_payload(&event()),
        )
        .unwrap();
        assert_eq!(rendered, "alice");
    }

    #[test]
    fn missing_author_on_payload_is_a_type_error_even_without_annotation() {
        let error = search_and_replace_special_annotations(
            "plain",
            Some(&json!({})),
            &EventAttributes::from_payload(&event()),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'login')"
        );
    }

    const UPSTREAM_SPECIAL_ANNOTATIONS: [(&str, &str); 5] = [
        ("@author", "creator"),
        ("@action", "created"),
        ("@sender", "initiator"),
        ("@bot", "Mergeable[bot]"),
        ("@repository", "botrepo"),
    ];

    fn assert_each_annotation_is_replaced(message: &str, expected: &str) {
        let payload = json!({"user": {"login": "creator"}});
        let event = json!({"action": "created", "repository": {"full_name": "botrepo"}, "sender": {"login": "initiator"}});
        for (annotation, replacement) in UPSTREAM_SPECIAL_ANNOTATIONS {
            let message_with_annotation = message.replacen("$annotation$", annotation, 1);
            let message_with_replacement = expected.replacen("$annotation$", replacement, 1);
            let rendered = search_and_replace_special_annotations(
                &message_with_annotation,
                Some(&payload),
                &EventAttributes::from_payload(&event),
            )
            .unwrap();
            assert_eq!(rendered, message_with_replacement, "{annotation}");
        }
    }

    #[test]
    fn does_not_affect_input_if_no_special_annotations_are_found() {
        assert_each_annotation_is_replaced("no special annotations", "no special annotations");
    }

    #[test]
    fn special_annotation_at_the_beginning_of_string_works_properly() {
        assert_each_annotation_is_replaced("$annotation$ says hello!", "$annotation$ says hello!");
    }

    #[test]
    fn escape_character_works_properly() {
        assert_each_annotation_is_replaced("this is \\@author", "this is @author");
    }

    #[test]
    fn special_annotation_at_the_end_of_string_works_properly() {
        assert_each_annotation_is_replaced("this is $annotation$", "this is $annotation$");
    }

    #[test]
    fn double_at_annotation_is_replaced_and_prepending_at_remains() {
        assert_each_annotation_is_replaced("this is @$annotation$", "this is @$annotation$");
    }

    #[test]
    fn replaces_special_annotation_anywhere_in_the_text() {
        assert_each_annotation_is_replaced(
            "this is something$annotation$ speaking",
            "this is something$annotation$ speaking",
        );
    }
}
