use crate::error::{JsError, JsResult};
use crate::validators::options::lib::js_regex::JsRegex;

fn fixed_regex_matches(pattern: &str, text: &str) -> bool {
    JsRegex::new(pattern, "i").is_ok_and(|regex| regex.test(text))
}

fn compile_case_insensitive(pattern: &str) -> JsResult<JsRegex> {
    JsRegex::new(pattern, "i").map_err(|_| {
        JsError::named(
            "SyntaxError",
            format!("Invalid regular expression: /{pattern}/i"),
        )
    })
}

fn file_name(path: &str) -> Option<String> {
    let regex = JsRegex::new("\\/\\w+[.](\\w*)$", "i").ok()?;
    regex
        .exec_first_match(path)
        .map(|matched| matched[1..].to_string())
}

fn create_regex_to_match(pattern: &str) -> JsResult<JsRegex> {
    let mut regex_string = String::new();
    let mut to_match = pattern.to_string();
    if let Some(rooted) = to_match.strip_prefix('/') {
        to_match = rooted.to_string();
        regex_string.push('^');
    }
    let match_one_directory = fixed_regex_matches("\\/[*]$", &to_match);
    if match_one_directory {
        to_match.pop();
    }
    if fixed_regex_matches("\\/[*]{2}\\/", &to_match) {
        to_match = to_match.replacen("/**/", "\\/(\\w*\\/)*", 1);
    }
    if fixed_regex_matches("\\/[*]{2}$", &to_match) {
        to_match = to_match.replacen("/**", "\\/", 1);
    }
    if fixed_regex_matches("^[*]{2}\\/", &to_match) {
        to_match = to_match.replacen("**/", "", 1);
    }
    regex_string.push_str(&to_match);
    if match_one_directory {
        regex_string.push_str("\\w*[.]?\\w*$");
    }
    compile_case_insensitive(&regex_string)
}

pub fn check_if_path_matches(path: &str, to_match: &str) -> JsResult<bool> {
    if fixed_regex_matches("^[*]$", to_match) {
        return Ok(true);
    }
    if fixed_regex_matches("^[*][\\.]\\w*$", to_match) {
        let file = file_name(path).unwrap_or_else(|| "null".to_string());
        let extension_pattern: String = to_match.chars().skip(1).collect();
        return Ok(compile_case_insensitive(&extension_pattern)?.test(&file));
    }
    let regex = create_regex_to_match(to_match)?;
    let relative_path = path.strip_prefix('/').unwrap_or(path);
    Ok(regex.test(relative_path))
}

#[derive(Debug, Clone, PartialEq)]
pub struct OwnerFile {
    entries: Vec<Vec<String>>,
}

impl OwnerFile {
    pub fn owners_for(&self, path: &str) -> JsResult<Vec<String>> {
        let mut owners = Vec::new();
        for entry in &self.entries {
            let globs = entry.first().map(String::as_str).unwrap_or_default();
            let mut is_matching = globs.is_empty();
            if !is_matching {
                for glob in globs.split(' ') {
                    if check_if_path_matches(path, glob)? {
                        is_matching = !glob.is_empty();
                        break;
                    }
                }
            }
            if is_matching {
                owners = entry[1..].to_vec();
            }
        }
        Ok(owners)
    }
}

fn is_js_trim_whitespace(character: char) -> bool {
    crate::validators::options::lib::is_js_whitespace(character)
}

pub fn parse_owner_file(content: &str) -> OwnerFile {
    let entries = content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .map(|line| line.trim_matches(is_js_trim_whitespace))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.split(' ')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .collect();
    OwnerFile { entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(codeowners: &str, path: &str) -> Vec<String> {
        parse_owner_file(codeowners).owners_for(path).unwrap()
    }

    #[test]
    fn star_matches_every_path() {
        assert_eq!(
            owners("* @bob", "first/second/third/dir/test.js"),
            vec!["@bob"]
        );
        assert_eq!(owners("* @bob", "another/file/path/test.js"), vec!["@bob"]);
    }

    #[test]
    fn file_type_pattern_matches_the_extension() {
        assert_eq!(
            owners("*.js @bob", "first/second/third/dir/test.js"),
            vec!["@bob"]
        );
        assert!(owners("*.go @bob", "another/file/path/test.js").is_empty());
        assert_eq!(
            owners("*.go @bob", "another/file/path/test.go"),
            vec!["@bob"]
        );
    }

    #[test]
    fn rooted_directory_matches_only_from_the_root() {
        assert_eq!(owners(" /docs/ @bob", "/docs/test.js"), vec!["@bob"]);
        assert!(owners(" /docs/ @bob", "another/file/docs/test.js").is_empty());
        assert_eq!(
            owners("/docs/ @bob", "/docs/long/path/to/test.js"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("/docs/ @bob", "docs/test/anotherTest.js"),
            vec!["@bob"]
        );
    }

    #[test]
    fn directory_star_matches_only_direct_files() {
        assert_eq!(owners("/docs/* @bob", "/docs/test.js"), vec!["@bob"]);
        assert!(owners("/docs/* @bob", "/docs/not/matching/test.js").is_empty());
    }

    #[test]
    fn double_star_prefix_and_middle_match_nested_directories() {
        assert_eq!(
            owners("**/docs @bob", "first/second/docs/test.js"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("**/docs @bob", "first/second/docs/inside/test.js"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("/docs/**/apps/ @bob", "/docs/second/third/apps/test.js"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("/docs/**/apps/ @bob", "/docs/third/apps/inside/test.js"),
            vec!["@bob"]
        );
    }

    #[test]
    fn last_matching_line_wins_and_comments_are_skipped() {
        let codeowners = "# comment\r\n* @default\n*.js @js-owner @other";
        assert_eq!(
            owners(codeowners, "src/app.js"),
            vec!["@js-owner", "@other"]
        );
        assert_eq!(owners(codeowners, "README.md"), vec!["@default"]);
    }
}
