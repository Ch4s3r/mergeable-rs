use crate::context::Context;
use crate::error::{JsError, JsResult};
use crate::github::api;
use crate::js;
use crate::validators::options::lib::js_regex::JsRegex;
use serde_json::Value;

const CLOSES_ISSUE_PATTERN: &str =
    "\\b(closes?|closed|fix|fixes?|fixed|resolves?|resolved)\\b.#[0-9]*";

pub fn check_if_closes_an_issue(description: Option<&Value>) -> Vec<String> {
    let Ok(closes_issue_regex) = JsRegex::new(CLOSES_ISSUE_PATTERN, "ig") else {
        return Vec::new();
    };
    let text = js::to_js_string(description);
    let mut issues = Vec::new();
    while let Some((start, end)) = closes_issue_regex.find(&text) {
        let matched = &text[start..end];
        if let Some(hash_position) = matched.find('#') {
            issues.push(matched[hash_position + 1..].to_string());
        }
    }
    issues
}

pub async fn check_if_issue_have_property(
    context: &Context,
    issues: &[String],
    property: &str,
) -> JsResult<Vec<Value>> {
    let mut output = Vec::new();
    for issue_number in issues {
        let issue = api::get_issue(context, &Value::String(issue_number.clone()))
            .await?
            .ok_or_else(|| JsError::reading_property_of_undefined("data"))?;
        if let Some(value) = issue
            .data
            .get(property)
            .filter(|value| js::is_truthy(value))
        {
            output.push(value.clone());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn closing_keywords_reference_issue_numbers() {
        let body = json!("This Fixes #12 and closes #3, also resolved: #44 but not prefix#5");
        assert_eq!(check_if_closes_an_issue(Some(&body)), vec!["12", "3"]);
    }

    #[test]
    fn missing_body_closes_nothing() {
        assert!(check_if_closes_an_issue(None).is_empty());
        assert!(check_if_closes_an_issue(Some(&Value::Null)).is_empty());
    }
}
