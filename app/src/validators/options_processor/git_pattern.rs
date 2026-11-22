use crate::error::{Error, ErrorKind, Result};
use crate::validators::options::text_matching::is_trimmable_whitespace;
use crate::validators::options::user_pattern::UserPattern;

fn compile_case_insensitive(pattern: &str) -> Result<UserPattern> {
    UserPattern::new(pattern, "i").map_err(|_| {
        Error::new(
            ErrorKind::InvalidSyntax,
            format!("Invalid regular expression: /{pattern}/i"),
        )
    })
}

fn is_ascii_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn is_extension_glob(glob: &str) -> bool {
    glob.strip_prefix("*.")
        .is_some_and(|extension| extension.chars().all(is_ascii_word_character))
}

fn file_name(path: &str) -> Option<&str> {
    let (_, last_segment) = path.rsplit_once('/')?;
    let (stem, extension) = last_segment.split_once('.')?;
    let is_word = |text: &str| text.chars().all(is_ascii_word_character);
    (!stem.is_empty() && is_word(stem) && is_word(extension)).then_some(last_segment)
}

fn create_regex_to_match(pattern: &str) -> Result<UserPattern> {
    let mut regex_string = String::new();
    let mut to_match = pattern.to_string();
    if let Some(rooted) = to_match.strip_prefix('/') {
        to_match = rooted.to_string();
        regex_string.push('^');
    }
    let match_one_directory = to_match.ends_with("/*");
    if match_one_directory {
        to_match.pop();
    }
    if to_match.contains("/**/") {
        to_match = to_match.replacen("/**/", "\\/(\\w*\\/)*", 1);
    }
    if to_match.ends_with("/**") {
        to_match = to_match.replacen("/**", "\\/", 1);
    }
    if to_match.starts_with("**/") {
        to_match = to_match.replacen("**/", "", 1);
    }
    regex_string.push_str(&to_match);
    if match_one_directory {
        regex_string.push_str("\\w*[.]?\\w*$");
    }
    compile_case_insensitive(&regex_string)
}

enum GlobMatcher {
    Everything,
    Extension(Result<UserPattern>),
    Path(Result<UserPattern>),
}

impl GlobMatcher {
    fn new(glob: &str) -> Self {
        if glob == "*" {
            GlobMatcher::Everything
        } else if let Some(extension) = glob.strip_prefix('*').filter(|_| is_extension_glob(glob)) {
            GlobMatcher::Extension(compile_case_insensitive(extension))
        } else {
            GlobMatcher::Path(create_regex_to_match(glob))
        }
    }

    fn matches(&self, path: &str) -> Result<bool> {
        match self {
            GlobMatcher::Everything => Ok(true),
            GlobMatcher::Extension(extension_regex) => {
                let extension_regex = extension_regex.as_ref().map_err(Clone::clone)?;
                Ok(extension_regex.is_match(file_name(path).unwrap_or("null")))
            }
            GlobMatcher::Path(path_regex) => {
                let path_regex = path_regex.as_ref().map_err(Clone::clone)?;
                let relative_path = path.strip_prefix('/').unwrap_or(path);
                Ok(path_regex.is_match(relative_path))
            }
        }
    }
}

pub fn check_if_path_matches(path: &str, to_match: &str) -> Result<bool> {
    GlobMatcher::new(to_match).matches(path)
}

struct OwnerEntry {
    globs: Vec<(bool, GlobMatcher)>,
    owners: Vec<String>,
}

impl OwnerEntry {
    fn matches(&self, path: &str) -> Result<bool> {
        if self.globs.is_empty() {
            return Ok(true);
        }
        for (is_empty_glob, matcher) in &self.globs {
            if matcher.matches(path)? {
                return Ok(!is_empty_glob);
            }
        }
        Ok(false)
    }
}

pub struct OwnerFile {
    entries: Vec<OwnerEntry>,
}

impl OwnerFile {
    pub fn owners_for(&self, path: &str) -> Result<&[String]> {
        let mut owners: &[String] = &[];
        for entry in &self.entries {
            if entry.matches(path)? {
                owners = &entry.owners;
            }
        }
        Ok(owners)
    }
}

fn parse_owner_entry(line: &str) -> OwnerEntry {
    let mut parts = line.split(' ').filter(|part| !part.is_empty());
    let globs = parts
        .next()
        .filter(|globs| !globs.is_empty())
        .map(|globs| {
            globs
                .split(' ')
                .map(|glob| (glob.is_empty(), GlobMatcher::new(glob)))
                .collect()
        })
        .unwrap_or_default();
    OwnerEntry {
        globs,
        owners: parts.map(str::to_string).collect(),
    }
}

pub fn parse_owner_file(content: &str) -> OwnerFile {
    let entries = content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .map(|line| line.trim_matches(is_trimmable_whitespace))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(parse_owner_entry)
        .collect();
    OwnerFile { entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(codeowners: &str, path: &str) -> Vec<String> {
        parse_owner_file(codeowners)
            .owners_for(path)
            .unwrap()
            .to_vec()
    }

    #[test]
    fn string_pattern_checks_classify_sample_paths() {
        let file_name_regex = UserPattern::new("\\/\\w+[.](\\w*)$", "i").unwrap();
        for path in [
            "a/b/test.rs",
            "a/b.c/d",
            "a/x.y.z",
            "/.rs",
            "a/_x.",
            "noslash.rs",
            "a/b/c",
        ] {
            let expected = file_name_regex
                .first_match(path)
                .map(|matched| matched[1..].to_string());
            assert_eq!(file_name(path).map(str::to_string), expected, "{path}");
        }
        let extension_regex = UserPattern::new("^[*][\\.]\\w*$", "i").unwrap();
        for glob in ["*.rs", "*.", "*.r-s", "*.rs/", "**.rs", "*"] {
            assert_eq!(
                is_extension_glob(glob),
                extension_regex.is_match(glob),
                "{glob}"
            );
        }
    }

    #[test]
    fn star_matches_every_path() {
        assert_eq!(
            owners("* @bob", "first/second/third/dir/test.rs"),
            vec!["@bob"]
        );
        assert_eq!(owners("* @bob", "another/file/path/test.rs"), vec!["@bob"]);
    }

    #[test]
    fn file_type_pattern_matches_the_extension() {
        assert_eq!(
            owners("*.rs @bob", "first/second/third/dir/test.rs"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("*.go @bob", "another/file/path/test.rs"),
            [] as [std::string::String; 0]
        );
        assert_eq!(
            owners("*.go @bob", "another/file/path/test.go"),
            vec!["@bob"]
        );
    }

    #[test]
    fn rooted_directory_matches_only_from_the_root() {
        assert_eq!(owners(" /docs/ @bob", "/docs/test.rs"), vec!["@bob"]);
        assert_eq!(
            owners(" /docs/ @bob", "another/file/docs/test.rs"),
            [] as [std::string::String; 0]
        );
        assert_eq!(
            owners("/docs/ @bob", "/docs/long/path/to/test.rs"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("/docs/ @bob", "docs/test/anotherTest.rs"),
            vec!["@bob"]
        );
    }

    #[test]
    fn directory_star_matches_only_direct_files() {
        assert_eq!(owners("/docs/* @bob", "/docs/test.rs"), vec!["@bob"]);
        assert_eq!(
            owners("/docs/* @bob", "/docs/not/matching/test.rs"),
            [] as [std::string::String; 0]
        );
    }

    #[test]
    fn double_star_prefix_and_middle_match_nested_directories() {
        assert_eq!(
            owners("**/docs @bob", "first/second/docs/test.rs"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("**/docs @bob", "first/second/docs/inside/test.rs"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("/docs/**/apps/ @bob", "/docs/second/third/apps/test.rs"),
            vec!["@bob"]
        );
        assert_eq!(
            owners("/docs/**/apps/ @bob", "/docs/third/apps/inside/test.rs"),
            vec!["@bob"]
        );
    }

    #[test]
    fn last_matching_line_wins_and_comments_are_skipped() {
        let codeowners = "# comment\r\n* @default\n*.rs @script-owner @other";
        assert_eq!(
            owners(codeowners, "src/app.rs"),
            vec!["@script-owner", "@other"]
        );
        assert_eq!(owners(codeowners, "README.md"), vec!["@default"]);
    }
}
