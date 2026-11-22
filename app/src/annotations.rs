use crate::error::JsResult;
use crate::js;
use serde_json::Value;

const ANNOTATIONS: [&str; 5] = ["@author", "@action", "@bot", "@repository", "@sender"];

pub fn bot_name() -> String {
    match std::env::var("APP_NAME") {
        Ok(name) if !name.is_empty() => format!("{name}[bot]"),
        _ => "Mergeable[bot]".to_string(),
    }
}

fn annotation_value(annotation: &str, payload: Option<&Value>, event: &Value) -> JsResult<String> {
    Ok(match annotation {
        "@author" => js::to_js_string(js::read(js::read(payload, "user")?, "login")?),
        "@action" => js::to_js_string(event.get("action")),
        "@bot" => bot_name(),
        "@repository" => match js::get_path(event, &["repository", "full_name"]) {
            None | Some(Value::Null) => String::new(),
            Some(full_name) => js::to_js_string(Some(full_name)),
        },
        _ => match js::read(event.get("sender"), "login")? {
            None | Some(Value::Null) => String::new(),
            Some(login) => js::to_js_string(Some(login)),
        },
    })
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
    event: &Value,
) -> JsResult<String> {
    let mut result = template.to_string();
    for annotation in ANNOTATIONS {
        let replacement = annotation_value(annotation, payload, event)?;
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
            &event(),
        )
        .unwrap();
        assert_eq!(rendered, "hi alice and @author by octocat in acme/app");
    }

    #[test]
    fn escaped_annotation_at_start_is_replaced_after_unescaping() {
        let payload = json!({"user": {"login": "alice"}});
        let rendered =
            search_and_replace_special_annotations("\\@author", Some(&payload), &event()).unwrap();
        assert_eq!(rendered, "alice");
    }

    #[test]
    fn missing_author_on_payload_is_a_type_error_even_without_annotation() {
        let error = search_and_replace_special_annotations("plain", Some(&json!({})), &event())
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'login')"
        );
    }
}
