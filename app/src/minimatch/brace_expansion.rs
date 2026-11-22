use regex::Regex;
use std::sync::LazyLock;

const ESCAPED_SLASH: &str = "\u{0}SLASH\u{0}";
const ESCAPED_OPEN: &str = "\u{0}OPEN\u{0}";
const ESCAPED_CLOSE: &str = "\u{0}CLOSE\u{0}";
const ESCAPED_COMMA: &str = "\u{0}COMMA\u{0}";
const ESCAPED_PERIOD: &str = "\u{0}PERIOD\u{0}";

static NUMERIC_SEQUENCE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^-?[0-9]+\.\.-?[0-9]+(?:\.\.-?[0-9]+)?$").ok());
static ALPHA_SEQUENCE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z]\.\.[a-zA-Z](?:\.\.-?[0-9]+)?$").ok());
static PADDED_NUMBER: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^-?0[0-9]").ok());
static COMMA_BEFORE_CLOSE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r",[^\n\r\u{2028}\u{2029}]*\}").ok());

fn is_match(pattern: &LazyLock<Option<Regex>>, text: &str) -> bool {
    pattern
        .as_ref()
        .is_some_and(|compiled| compiled.is_match(text))
}

struct BalancedMatch {
    pre: String,
    body: String,
    post: String,
}

fn balanced_range(text: &str) -> Option<(usize, usize)> {
    let mut open_index = text.find('{');
    let mut close_index = match open_index {
        Some(start) => text[start + 1..].find('}').map(|offset| offset + start + 1),
        None => text.find('}').filter(|index| *index > 0),
    };
    let (Some(first_open), Some(_)) = (open_index, close_index) else {
        return None;
    };
    let mut begins: Vec<usize> = Vec::new();
    let mut left = text.len();
    let mut right = 0;
    let mut result = None;
    let mut cursor = Some(first_open);
    while let Some(position) = cursor {
        if result.is_some() {
            break;
        }
        if Some(position) == open_index {
            begins.push(position);
            open_index = text[position + 1..]
                .find('{')
                .map(|offset| offset + position + 1);
        } else if begins.len() == 1 {
            result = begins.pop().zip(close_index);
        } else {
            if let Some(begin) = begins.pop()
                && begin < left
            {
                left = begin;
                right = close_index.unwrap_or(0);
            }
            close_index = text[position + 1..]
                .find('}')
                .map(|offset| offset + position + 1);
        }
        cursor = match (open_index, close_index) {
            (Some(open), Some(close)) if open < close => Some(open),
            (_, close) => close,
        };
    }
    if result.is_none() && !begins.is_empty() {
        result = Some((left, right));
    }
    result
}

fn balanced(text: &str) -> Option<BalancedMatch> {
    let (start, end) = balanced_range(text)?;
    Some(BalancedMatch {
        pre: text[..start].to_string(),
        body: text[start + 1..end].to_string(),
        post: text[end + 1..].to_string(),
    })
}

fn escape_braces(text: &str) -> String {
    text.split("\\\\")
        .collect::<Vec<_>>()
        .join(ESCAPED_SLASH)
        .split("\\{")
        .collect::<Vec<_>>()
        .join(ESCAPED_OPEN)
        .split("\\}")
        .collect::<Vec<_>>()
        .join(ESCAPED_CLOSE)
        .split("\\,")
        .collect::<Vec<_>>()
        .join(ESCAPED_COMMA)
        .split("\\.")
        .collect::<Vec<_>>()
        .join(ESCAPED_PERIOD)
}

fn unescape_braces(text: &str) -> String {
    text.replace(ESCAPED_SLASH, "\\")
        .replace(ESCAPED_OPEN, "{")
        .replace(ESCAPED_CLOSE, "}")
        .replace(ESCAPED_COMMA, ",")
        .replace(ESCAPED_PERIOD, ".")
}

fn parse_comma_parts(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let Some(found) = balanced(text) else {
        return text.split(',').map(str::to_string).collect();
    };
    let mut parts: Vec<String> = found.pre.split(',').map(str::to_string).collect();
    if let Some(last) = parts.last_mut() {
        last.push('{');
        last.push_str(&found.body);
        last.push('}');
    }
    let mut post_parts = parse_comma_parts(&found.post);
    if !found.post.is_empty() {
        if !post_parts.is_empty() {
            let first_post = post_parts.remove(0);
            if let Some(last) = parts.last_mut() {
                last.push_str(&first_post);
            }
        }
        parts.extend(post_parts);
    }
    parts
}

#[must_use]
pub fn expand_top(pattern: &str) -> Vec<String> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let mut text = pattern.to_string();
    if text.starts_with("{}") {
        text = format!("\\{{\\}}{}", &text[2..]);
    }
    expand(&escape_braces(&text), true)
        .iter()
        .map(|expansion| unescape_braces(expansion))
        .collect()
}

fn numeric(text: &str) -> i64 {
    match text.parse::<i64>() {
        Ok(number) => number,
        Err(_) => text
            .chars()
            .next()
            .map_or(0, |character| i64::from(u32::from(character))),
    }
}

fn expand(text: &str, is_top: bool) -> Vec<String> {
    let Some(found) = balanced(text) else {
        return vec![text.to_string()];
    };
    if found.pre.ends_with('$') {
        return vec![text.to_string()];
    }
    let is_numeric_sequence = is_match(&NUMERIC_SEQUENCE, &found.body);
    let is_alpha_sequence = is_match(&ALPHA_SEQUENCE, &found.body);
    let is_sequence = is_numeric_sequence || is_alpha_sequence;
    let is_options = found.body.contains(',');
    if !is_sequence && !is_options {
        if is_match(&COMMA_BEFORE_CLOSE, &found.post) {
            let rewritten = format!(
                "{}{{{}{}{}",
                found.pre, found.body, ESCAPED_CLOSE, found.post
            );
            return expand(&rewritten, false);
        }
        return vec![text.to_string()];
    }
    let parts: Vec<String> = if is_sequence {
        found.body.split("..").map(str::to_string).collect()
    } else {
        let comma_parts = parse_comma_parts(&found.body);
        if comma_parts.len() == 1 {
            let embraced: Vec<String> = expand(&comma_parts[0], false)
                .iter()
                .map(|inner| format!("{{{inner}}}"))
                .collect();
            if embraced.len() == 1 {
                let post = if found.post.is_empty() {
                    vec![String::new()]
                } else {
                    expand(&found.post, false)
                };
                return post
                    .iter()
                    .map(|suffix| format!("{}{}{}", found.pre, embraced[0], suffix))
                    .collect();
            }
            embraced
        } else {
            comma_parts
        }
    };
    let post = if found.post.is_empty() {
        vec![String::new()]
    } else {
        expand(&found.post, false)
    };
    let items: Vec<String> = if is_sequence {
        sequence_items(&parts, is_alpha_sequence)
    } else {
        parts.iter().flat_map(|part| expand(part, false)).collect()
    };
    let mut expansions = Vec::new();
    for item in &items {
        for suffix in &post {
            let expansion = format!("{}{}{}", found.pre, item, suffix);
            if !is_top || is_sequence || !expansion.is_empty() {
                expansions.push(expansion);
            }
        }
    }
    expansions
}

fn sequence_items(parts: &[String], is_alpha_sequence: bool) -> Vec<String> {
    let start = numeric(&parts[0]);
    let end = parts.get(1).map_or(start, |part| numeric(part));
    let width = parts[0].len().max(parts.get(1).map_or(0, String::len));
    let mut increment = if parts.len() == 3 {
        numeric(&parts[2]).abs()
    } else {
        1
    };
    let reverse = end < start;
    if reverse {
        increment = -increment;
    }
    let pad = parts.iter().any(|part| is_match(&PADDED_NUMBER, part));
    let mut items = Vec::new();
    if increment == 0 {
        // ponytail: brace-expansion loops forever on a zero step; return no items instead of hanging
        return items;
    }
    let mut current = start;
    while (!reverse && current <= end) || (reverse && current >= end) {
        let item = if is_alpha_sequence {
            let character = u32::try_from(current)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{fffd}');
            if character == '\\' {
                String::new()
            } else {
                character.to_string()
            }
        } else {
            let mut digits = current.to_string();
            if pad {
                let needed = width.saturating_sub(digits.len());
                if needed > 0 {
                    let zeros = "0".repeat(needed);
                    digits = if current < 0 {
                        format!("-{zeros}{}", &digits[1..])
                    } else {
                        format!("{zeros}{digits}")
                    };
                }
            }
            digits
        };
        items.push(item);
        current += increment;
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_sets_and_nested_sets_expand_like_bash() {
        assert_eq!(expand_top("a{b,c}d"), vec!["abd", "acd"]);
        assert_eq!(expand_top("a{b,c{d,e}f}g"), vec!["abg", "acdfg", "acefg"]);
        assert_eq!(expand_top("a{b,}c"), vec!["abc", "ac"]);
    }

    #[test]
    fn numeric_and_alphabetic_ranges_expand_with_padding() {
        assert_eq!(expand_top("a{0..3}d"), vec!["a0d", "a1d", "a2d", "a3d"]);
        assert_eq!(expand_top("{01..03}"), vec!["01", "02", "03"]);
        assert_eq!(expand_top("{c..a}"), vec!["c", "b", "a"]);
        assert_eq!(expand_top("{1..7..3}"), vec!["1", "4", "7"]);
    }

    #[test]
    fn invalid_sets_are_left_alone() {
        assert_eq!(expand_top("a{2..}b"), vec!["a{2..}b"]);
        assert_eq!(expand_top("a{b}c"), vec!["a{b}c"]);
        assert_eq!(expand_top("{a},b}"), vec!["a}", "b"]);
    }
}
