pub mod brace_expansion;
use crate::error::{JsError, JsResult};
use fancy_regex::Regex;
use serde_json::Value;
use std::fmt::Write as _;
use std::sync::{Arc, LazyLock};

const COMPILED_GLOB_CACHE_CAPACITY: u64 = 4096;

static COMPILED_GLOBS: LazyLock<moka::sync::Cache<(String, bool), Arc<Minimatch>>> =
    LazyLock::new(|| moka::sync::Cache::new(COMPILED_GLOB_CACHE_CAPACITY));

const QUESTION_MARK: &str = "[^/]";
const STAR: &str = "[^/]*?";
const REGEX_SPECIALS: &str = "().*{}+?[]^$\\!";
const MAX_PATTERN_LENGTH: usize = 1024 * 64;

enum PatternPart {
    Literal(String),
    Globstar,
    Compiled(Regex),
}

struct ParsedSegment {
    source: String,
    has_magic: bool,
}

struct PatternList {
    kind: char,
    regex_start: usize,
    open_length: usize,
    regex_end: usize,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the option and state flags of the JavaScript minimatch object"
)]
struct Minimatch {
    dot: bool,
    negate: bool,
    comment: bool,
    empty: bool,
    set: Vec<Vec<PatternPart>>,
}

pub fn minimatch(file_path: &Value, pattern: &Value, dot: bool) -> JsResult<bool> {
    let Value::String(pattern_text) = pattern else {
        return Err(JsError::type_error("invalid pattern"));
    };
    assert_pattern_length(pattern_text)?;
    if pattern_text.starts_with('#') {
        return Ok(false);
    }
    compiled_glob(pattern_text, dot).matches(file_path)
}

pub fn matches(file_path: &str, pattern: &str, dot: bool) -> JsResult<bool> {
    assert_pattern_length(pattern)?;
    if pattern.starts_with('#') {
        return Ok(false);
    }
    Ok(compiled_glob(pattern, dot).matches_path(file_path))
}

fn compiled_glob(pattern: &str, dot: bool) -> Arc<Minimatch> {
    COMPILED_GLOBS.get_with((pattern.to_string(), dot), || {
        Arc::new(Minimatch::new(pattern, dot))
    })
}

fn assert_pattern_length(pattern: &str) -> JsResult<()> {
    if pattern.encode_utf16().count() > MAX_PATTERN_LENGTH {
        Err(JsError::type_error("pattern is too long"))
    } else {
        Ok(())
    }
}

fn split_on_slashes(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut segment_start = 0;
    let mut previous_was_slash = false;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'/' {
            if !previous_was_slash {
                parts.push(&text[segment_start..index]);
            }
            previous_was_slash = true;
            segment_start = index + 1;
        } else {
            previous_was_slash = false;
        }
    }
    parts.push(&text[segment_start..]);
    parts
}

fn has_brace_set(pattern: &str) -> bool {
    let characters: Vec<char> = pattern.chars().collect();
    for (open_index, character) in characters.iter().enumerate() {
        if *character != '{' {
            continue;
        }
        for following in &characters[open_index + 1..] {
            match following {
                '}' => return true,
                '{' | '\n' | '\r' | '\u{2028}' | '\u{2029}' => break,
                _ => {}
            }
        }
    }
    false
}

fn glob_unescape(text: &str) -> String {
    let mut unescaped = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(next) = characters.peek().copied()
            && !matches!(next, '\n' | '\r' | '\u{2028}' | '\u{2029}')
        {
            unescaped.push(next);
            characters.next();
            continue;
        }
        unescaped.push(character);
    }
    unescaped
}

fn translate_class_body(body: &str) -> String {
    match body {
        "" => return r"^\s\S".to_string(),
        "^" => return r"\s\S".to_string(),
        _ => {}
    }
    let mut translated = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(next) if matches!(next, 'd' | 'D' | 'w' | 'W' | 's' | 'S') => {
                    translated.push('\\');
                    translated.push(next);
                }
                Some(next) => translated.push_str(&regex::escape(next.encode_utf8(&mut [0; 4]))),
                None => translated.push_str(r"\\"),
            },
            '[' | '&' | '~' => {
                translated.push_str(&regex::escape(character.encode_utf8(&mut [0; 4])));
            }
            other => translated.push(other),
        }
    }
    translated
}

fn javascript_source_to_rust(source: &str) -> Option<String> {
    let mut translated = String::new();
    let mut characters = source.chars();
    let mut class_body: Option<String> = None;
    while let Some(character) = characters.next() {
        if let Some(body) = class_body.as_mut() {
            match character {
                '\\' => {
                    body.push('\\');
                    if let Some(next) = characters.next() {
                        body.push(next);
                    }
                }
                ']' => {
                    translated.push('[');
                    translated.push_str(&translate_class_body(body));
                    translated.push(']');
                    class_body = None;
                }
                other => body.push(other),
            }
            continue;
        }
        match character {
            '\\' => match characters.next() {
                None => translated.push_str(r"\\"),
                Some(escaped) if escaped.is_ascii_alphanumeric() => {
                    translated.push('\\');
                    translated.push(escaped);
                }
                Some(escaped) => {
                    let _ = write!(translated, "\\x{{{:x}}}", escaped as u32);
                }
            },
            '[' => class_body = Some(String::new()),
            other => translated.push(other),
        }
    }
    class_body.is_none().then_some(translated)
}

fn compile(source: &str) -> Option<Regex> {
    Regex::new(&javascript_source_to_rust(source)?).ok()
}

fn is_valid_class(contents: &str) -> bool {
    compile(&format!("[{contents}]")).is_some()
}

fn escape_unclosed_list_pipes(tail: &str) -> String {
    let mut rewritten = String::new();
    let mut pending_backslashes: usize = 0;
    for character in tail.chars() {
        match character {
            '\\' => pending_backslashes += 1,
            '|' => {
                let untouched = pending_backslashes.saturating_sub(129);
                let matched = pending_backslashes - untouched;
                let escaped_count = if matched.is_multiple_of(2) {
                    2 * matched + 1
                } else {
                    2 * matched - 1
                };
                rewritten.extend(std::iter::repeat_n('\\', untouched + escaped_count));
                rewritten.push('|');
                pending_backslashes = 0;
            }
            other => {
                rewritten.extend(std::iter::repeat_n('\\', pending_backslashes));
                pending_backslashes = 0;
                rewritten.push(other);
            }
        }
    }
    rewritten.extend(std::iter::repeat_n('\\', pending_backslashes));
    rewritten
}

fn remove_first_close_paren(text: &str) -> String {
    match text.find(')') {
        None => text.to_string(),
        Some(position) => {
            let after = &text[position + 1..];
            let skip = usize::from(after.starts_with(['+', '*', '?']));
            format!("{}{}", &text[..position], &after[skip..])
        }
    }
}

impl Minimatch {
    fn new(raw_pattern: &str, dot: bool) -> Self {
        let mut matcher = Minimatch {
            dot,
            negate: false,
            comment: false,
            empty: false,
            set: Vec::new(),
        };
        matcher.make(crate::lodash::js_trim(raw_pattern));
        matcher
    }

    fn make(&mut self, pattern: &str) {
        if pattern.starts_with('#') {
            self.comment = true;
            return;
        }
        if pattern.is_empty() {
            self.empty = true;
            return;
        }
        let negation_count = pattern
            .chars()
            .take_while(|character| *character == '!')
            .count();
        self.negate = negation_count % 2 == 1;
        let pattern = &pattern[negation_count..];
        let brace_set = if has_brace_set(pattern) {
            brace_expansion::expand_top(pattern)
        } else {
            vec![pattern.to_string()]
        };
        for expanded in brace_set {
            let mut parts = Vec::new();
            let mut compiled_all = true;
            for segment in split_on_slashes(&expanded) {
                match self.parse_part(segment) {
                    Some(part) => parts.push(part),
                    None => compiled_all = false,
                }
            }
            if compiled_all {
                self.set.push(parts);
            }
        }
    }

    fn parse_part(&self, pattern: &str) -> Option<PatternPart> {
        if pattern == "**" {
            return Some(PatternPart::Globstar);
        }
        if pattern.is_empty() {
            return Some(PatternPart::Literal(String::new()));
        }
        let parsed = self.parse(pattern, false)?;
        if !parsed.has_magic {
            return Some(PatternPart::Literal(glob_unescape(pattern)));
        }
        let compiled = compile(&format!("^{}$", parsed.source)).or_else(|| compile("$."))?;
        Some(PatternPart::Compiled(compiled))
    }

    fn parse_class_remainder(&self, pattern: &str) -> Option<ParsedSegment> {
        if pattern.is_empty() || pattern == "**" {
            return Some(ParsedSegment {
                source: "undefined".to_string(),
                has_magic: false,
            });
        }
        self.parse(pattern, true)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "single-pass glob parser ported from minimatch; splitting the state machine would obscure it"
    )]
    fn parse(&self, pattern: &str, is_sub_parse: bool) -> Option<ParsedSegment> {
        let characters: Vec<char> = pattern.chars().collect();
        let mut regex_source = String::new();
        let mut has_magic = false;
        let mut escaping = false;
        let mut pattern_list_stack: Vec<PatternList> = Vec::new();
        let mut negative_lists: Vec<PatternList> = Vec::new();
        let mut state_char: Option<char> = None;
        let mut in_class = false;
        let mut regex_class_start = 0;
        let mut class_start: Option<usize> = None;
        let pattern_start = if characters.first() == Some(&'.') {
            ""
        } else if self.dot {
            r"(?!(?:^|\/)\.{1,2}(?:$|\/))"
        } else {
            r"(?!\.)"
        };

        let clear_state_char = |state: &mut Option<char>, source: &mut String, magic: &mut bool| {
            if let Some(pending) = state.take() {
                match pending {
                    '*' => {
                        source.push_str(STAR);
                        *magic = true;
                    }
                    '?' => {
                        source.push_str(QUESTION_MARK);
                        *magic = true;
                    }
                    other => {
                        source.push('\\');
                        source.push(other);
                    }
                }
            }
        };

        let mut index = 0;
        while index < characters.len() {
            let mut character = characters[index];
            if escaping && REGEX_SPECIALS.contains(character) {
                regex_source.push('\\');
                regex_source.push(character);
                escaping = false;
                index += 1;
                continue;
            }
            match character {
                '/' => return None,
                '\\' => {
                    clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
                    escaping = true;
                }
                '?' | '*' | '+' | '@' | '!' => {
                    if in_class {
                        if character == '!' && class_start.map(|start| start + 1) == Some(index) {
                            character = '^';
                        }
                        regex_source.push(character);
                    } else {
                        clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
                        state_char = Some(character);
                    }
                }
                '(' => {
                    if in_class {
                        regex_source.push('(');
                    } else if let Some(kind) = state_char.take() {
                        let open = if kind == '!' { "(?:(?!(?:" } else { "(?:" };
                        pattern_list_stack.push(PatternList {
                            kind,
                            regex_start: regex_source.len(),
                            open_length: open.len(),
                            regex_end: 0,
                        });
                        regex_source.push_str(open);
                    } else {
                        regex_source.push_str(r"\(");
                    }
                }
                ')' => {
                    if in_class || pattern_list_stack.is_empty() {
                        regex_source.push_str(r"\)");
                    } else {
                        clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
                        has_magic = true;
                        if let Some(mut list) = pattern_list_stack.pop() {
                            regex_source.push_str(match list.kind {
                                '!' => "))[^/]*?)",
                                '?' => ")?",
                                '+' => ")+",
                                '*' => ")*",
                                _ => ")",
                            });
                            list.regex_end = regex_source.len();
                            if list.kind == '!' {
                                negative_lists.push(list);
                            }
                        }
                    }
                }
                '|' => {
                    if in_class || pattern_list_stack.is_empty() || escaping {
                        regex_source.push_str(r"\|");
                        escaping = false;
                    } else {
                        clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
                        regex_source.push('|');
                    }
                }
                '[' => {
                    clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
                    if in_class {
                        regex_source.push_str(r"\[");
                    } else {
                        in_class = true;
                        class_start = Some(index);
                        regex_class_start = regex_source.len();
                        regex_source.push('[');
                    }
                }
                ']' => {
                    let start = class_start.unwrap_or(0);
                    if class_start.map(|start| start + 1) == Some(index) || !in_class {
                        regex_source.push_str(r"\]");
                        escaping = false;
                    } else {
                        let class_contents: String = characters[start + 1..index].iter().collect();
                        if is_valid_class(&class_contents) {
                            has_magic = true;
                            in_class = false;
                            regex_source.push(']');
                        } else {
                            let sub = self.parse_class_remainder(&class_contents)?;
                            regex_source.truncate(regex_class_start);
                            regex_source.push_str(r"\[");
                            regex_source.push_str(&sub.source);
                            regex_source.push_str(r"\]");
                            has_magic = has_magic || sub.has_magic;
                            in_class = false;
                        }
                    }
                }
                _ => {
                    clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
                    if escaping {
                        escaping = false;
                    } else if REGEX_SPECIALS.contains(character) && !(character == '^' && in_class)
                    {
                        regex_source.push('\\');
                    }
                    regex_source.push(character);
                }
            }
            index += 1;
        }

        if in_class {
            let start = class_start.unwrap_or(0);
            let class_contents: String = characters[start + 1..].iter().collect();
            let sub = self.parse_class_remainder(&class_contents)?;
            regex_source.truncate(regex_class_start);
            regex_source.push_str(r"\[");
            regex_source.push_str(&sub.source);
            has_magic = has_magic || sub.has_magic;
        }

        while let Some(list) = pattern_list_stack.pop() {
            let tail = escape_unclosed_list_pipes(
                regex_source.get(list.regex_start + list.open_length..)?,
            );
            let replacement = match list.kind {
                '*' => STAR.to_string(),
                '?' => QUESTION_MARK.to_string(),
                other => format!("\\{other}"),
            };
            has_magic = true;
            regex_source = format!(
                "{}{}\\({}",
                regex_source.get(..list.regex_start)?,
                replacement,
                tail
            );
        }

        clear_state_char(&mut state_char, &mut regex_source, &mut has_magic);
        if escaping {
            regex_source.push_str(r"\\");
        }

        let add_pattern_start = matches!(regex_source.chars().next(), Some('[' | '.' | '('));

        for negative in negative_lists.iter().rev() {
            let last_start = negative.regex_end.checked_sub(8)?;
            let before = regex_source.get(..negative.regex_start)?.to_string();
            let first = regex_source
                .get(negative.regex_start..last_start)?
                .to_string();
            let mut last = regex_source
                .get(last_start..negative.regex_end)?
                .to_string();
            let mut after = regex_source.get(negative.regex_end..)?.to_string();
            last.push_str(&after);
            let open_parens_before = before.matches('(').count();
            for _ in 0..open_parens_before {
                after = remove_first_close_paren(&after);
            }
            let dollar = if after.is_empty() && !is_sub_parse {
                "$"
            } else {
                ""
            };
            regex_source = format!("{before}{first}{after}{dollar}{last}");
        }

        if !regex_source.is_empty() && has_magic {
            regex_source = format!("(?=.){regex_source}");
        }
        if add_pattern_start {
            regex_source = format!("{pattern_start}{regex_source}");
        }
        Some(ParsedSegment {
            source: regex_source,
            has_magic,
        })
    }

    fn matches(&self, file_path: &Value) -> JsResult<bool> {
        if self.comment {
            return Ok(false);
        }
        if self.empty {
            return Ok(file_path.as_str() == Some(""));
        }
        match file_path {
            Value::String(path_text) => Ok(self.matches_path(path_text)),
            Value::Null => Err(JsError::type_error(
                "Cannot read properties of null (reading 'split')",
            )),
            _ => Err(JsError::type_error("f.split is not a function")),
        }
    }

    fn matches_path(&self, path_text: &str) -> bool {
        if self.comment {
            return false;
        }
        if self.empty {
            return path_text.is_empty();
        }
        let file_segments = split_on_slashes(path_text);
        let any_pattern_matches = self
            .set
            .iter()
            .any(|pattern| self.match_one(&file_segments, pattern));
        any_pattern_matches != self.negate
    }

    fn match_one(&self, file: &[&str], pattern: &[PatternPart]) -> bool {
        let mut file_index = 0;
        let mut pattern_index = 0;
        while file_index < file.len() && pattern_index < pattern.len() {
            let segment = file[file_index];
            match &pattern[pattern_index] {
                PatternPart::Globstar => {
                    let rest_of_pattern = pattern_index + 1;
                    if rest_of_pattern == pattern.len() {
                        return file[file_index..].iter().all(|remaining| {
                            *remaining != "."
                                && *remaining != ".."
                                && (self.dot || !remaining.starts_with('.'))
                        });
                    }
                    let mut swallow_index = file_index;
                    while swallow_index < file.len() {
                        if self.match_one(&file[swallow_index..], &pattern[rest_of_pattern..]) {
                            return true;
                        }
                        let swallowed = file[swallow_index];
                        if swallowed == "."
                            || swallowed == ".."
                            || (!self.dot && swallowed.starts_with('.'))
                        {
                            break;
                        }
                        swallow_index += 1;
                    }
                    return false;
                }
                PatternPart::Literal(literal) => {
                    if segment != literal.as_str() {
                        return false;
                    }
                }
                PatternPart::Compiled(regex) => {
                    if !regex.is_match(segment).unwrap_or(false) {
                        return false;
                    }
                }
            }
            file_index += 1;
            pattern_index += 1;
        }
        if file_index == file.len() && pattern_index == pattern.len() {
            return true;
        }
        if file_index == file.len() {
            return false;
        }
        file_index == file.len() - 1 && file[file_index].is_empty()
    }
}

#[cfg(test)]
mod tests {

    const MINIMATCH_CORPUS: &str = include_str!("../../tests/fixtures/minimatch.json");

    #[test]
    fn every_corpus_glob_matches_like_minimatch_three() {
        let corpus: Vec<Value> = serde_json::from_str(MINIMATCH_CORPUS).unwrap();
        let mismatches: Vec<&Value> = corpus
            .iter()
            .filter(|entry| {
                let outcome = minimatch(&entry["file"], &entry["pattern"], false);
                match (entry.get("matches").and_then(Value::as_bool), &outcome) {
                    (Some(expected), Ok(actual)) => expected != *actual,
                    (None, Err(error)) => {
                        entry["error"].as_str() != Some(error.to_string().as_str())
                    }
                    _ => true,
                }
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "{} mismatches: {:?}",
            mismatches.len(),
            mismatches
        );
    }
    use super::*;

    fn glob(file_path: &str, pattern: &str) -> bool {
        matches(file_path, pattern, false).unwrap_or(false)
    }

    fn glob_with_dot(file_path: &str, pattern: &str) -> bool {
        matches(file_path, pattern, true).unwrap_or(false)
    }

    #[test]
    fn dotfiles_are_only_matched_with_the_dot_option() {
        assert!(!glob(".github/x.yml", "**/*.yml"));
        assert!(glob_with_dot(".github/x.yml", "**/*.yml"));
        assert!(!glob(".env", "*"));
        assert!(glob_with_dot(".env", "*"));
        assert!(glob_with_dot(".test/thing.js", "**"));
    }

    #[test]
    fn negation_comments_extglobs_and_braces_follow_minimatch() {
        assert!(glob("bug", "!feat*"));
        assert!(!glob("#x", "#x"));
        assert!(glob("aab", "+(a)b"));
        assert!(glob("a1", "a{1..3}"));
        assert!(glob("a/b", "a//b"));
        assert!(glob("ab", "a[!c]"));
        assert!(glob("ab", "a[^c]"));
        assert!(glob("x.js", "*.!(json)"));
        assert!(!glob("x.json", "*.!(json)"));
    }

    #[test]
    fn globstar_and_single_star_respect_path_segments() {
        assert!(!glob("src/x/a.js", "src/*.js"));
        assert!(glob("src/a.js", "src/**/*.js"));
        assert!(glob("c.js", "**/c.js"));
        assert!(glob("a/b", "a/**"));
        assert!(!glob("a", "a/**"));
        assert!(!glob("a.JS", "*.js"));
        assert!(!glob("a/b", "b"));
        assert!(glob("package.json", "package.json"));
        assert!(glob("test/test.js", "**/*.js"));
        assert!(glob("nested/two/three/big_file_3.js", "nested/two/**"));
        assert!(!glob("nested/two/three/big_file_3.js", "nested/one/*"));
    }

    #[test]
    fn unterminated_and_odd_character_classes_follow_minimatch() {
        assert!(glob("x[undefined", "*["));
        assert!(glob("[x]", "[[]x]"));
        assert!(glob("a", "[!]"));
        assert!(glob("!", "[!]"));
        assert!(!glob("a", "a[^]"));
        assert!(!glob("ab", "a[]b"));
        assert!(!glob("a]", "a[]"));
        assert!(glob("x[**", "x[**"));
        assert!(!glob("x[undefined", "x[**"));
        assert!(glob("x[a", "x[a"));
        assert!(!glob("xa", "x[a"));
    }

    #[test]
    fn non_string_patterns_are_rejected_like_minimatch() {
        let error = minimatch(&Value::String("a".to_string()), &Value::Null, false).unwrap_err();
        assert_eq!(error.to_string(), "TypeError: invalid pattern");
    }
}
