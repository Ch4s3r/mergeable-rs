use crate::context::Context;
use crate::error::{Error, Result};
use crate::github::api;
use crate::json_value;
use crate::validators::options::lib::compile_constant_pattern;
use crate::validators::options::lib::user_pattern::PatternFlags;
use serde_json::Value;
use std::sync::LazyLock;

static CLOSES_ISSUE_REGEX: LazyLock<Option<fancy_regex::Regex>> = LazyLock::new(|| {
    compile_constant_pattern(
        "\\b(closes?|closed|fix|fixes?|fixed|resolves?|resolved)\\b.#[0-9]*",
        PatternFlags {
            ignore_case: true,
            ..PatternFlags::default()
        },
    )
});

pub fn check_if_closes_an_issue(description: Option<&Value>) -> Vec<String> {
    let Some(closes_issue_regex) = CLOSES_ISSUE_REGEX.as_ref() else {
        return Vec::new();
    };
    let text = json_value::to_display_string(description);
    closes_issue_regex
        .find_iter(&text)
        .map_while(Result::ok)
        .filter_map(|matched| {
            let (_, issue_number) = matched.as_str().split_once('#')?;
            Some(issue_number.to_string())
        })
        .collect()
}

pub async fn check_if_issue_have_property(
    context: &Context,
    issues: &[String],
    property: &str,
) -> Result<Vec<Value>> {
    let mut output = Vec::new();
    for issue_number in issues {
        let mut issue = api::get_issue(context, &Value::String(issue_number.clone()))
            .await?
            .ok_or_else(|| Error::missing_property("data"))?;
        if let Some(value) = issue
            .data
            .get_mut(property)
            .filter(|value| json_value::is_truthy(value))
        {
            output.push(value.take());
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
        assert_eq!(
            check_if_closes_an_issue(None),
            [] as [std::string::String; 0]
        );
        assert_eq!(
            check_if_closes_an_issue(Some(&Value::Null)),
            [] as [std::string::String; 0]
        );
    }
}
