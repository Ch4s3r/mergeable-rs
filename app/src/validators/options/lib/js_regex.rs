use std::cell::Cell;

const WORD_CLASS_ITEMS: &str = "0-9A-Za-z_";
const DIGIT_CLASS_ITEMS: &str = "0-9";
const WHITESPACE_CLASS_ITEMS: &str = "\\t\\n\\x0B\\x0C\\r \\x{A0}\\x{1680}\\x{2000}-\\x{200A}\\x{2028}\\x{2029}\\x{202F}\\x{205F}\\x{3000}\\x{FEFF}";
const LINE_TERMINATOR_CLASS_ITEMS: &str = "\\n\\r\\x{2028}\\x{2029}";
const NEVER_MATCHING_CLASS: &str = "[^\\x00-\\x{10FFFF}]";

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct JsRegexFlags {
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub sticky: bool,
}

impl JsRegexFlags {
    pub fn parse(flags: &str) -> Option<Self> {
        let mut parsed = JsRegexFlags::default();
        let mut seen = String::new();
        for flag in flags.chars() {
            if seen.contains(flag) {
                return None;
            }
            seen.push(flag);
            match flag {
                'g' => parsed.global = true,
                'i' => parsed.ignore_case = true,
                'm' => parsed.multiline = true,
                's' => parsed.dot_all = true,
                'u' | 'v' => {
                    if parsed.unicode {
                        return None;
                    }
                    parsed.unicode = true;
                }
                'y' => parsed.sticky = true,
                'd' => {}
                _ => return None,
            }
        }
        Some(parsed)
    }
}

#[derive(Debug)]
pub struct JsRegex {
    compiled: fancy_regex::Regex,
    flags: JsRegexFlags,
    last_index: Cell<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsRegexSyntaxError;

impl JsRegex {
    pub fn new(pattern: &str, flags: &str) -> Result<Self, JsRegexSyntaxError> {
        let parsed_flags = JsRegexFlags::parse(flags).ok_or(JsRegexSyntaxError)?;
        let translated = translate(pattern, parsed_flags)?;
        let compiled = fancy_regex::Regex::new(&translated).map_err(|_| JsRegexSyntaxError)?;
        Ok(JsRegex {
            compiled,
            flags: parsed_flags,
            last_index: Cell::new(0),
        })
    }

    pub fn test(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    pub fn exec_first_match(&self, text: &str) -> Option<String> {
        self.find(text)
            .map(|(start, end)| text[start..end].to_string())
    }

    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        if !self.flags.global && !self.flags.sticky {
            return self.find_from(text, 0);
        }
        let last_index = self.last_index.get();
        if last_index > text.encode_utf16().count() {
            self.last_index.set(0);
            return None;
        }
        let start_byte = utf16_index_to_byte_offset(text, last_index);
        match self.find_from(text, start_byte) {
            Some((start, end)) if !self.flags.sticky || start == start_byte => {
                self.last_index.set(text[..end].encode_utf16().count());
                Some((start, end))
            }
            _ => {
                self.last_index.set(0);
                None
            }
        }
    }

    fn find_from(&self, text: &str, start_byte: usize) -> Option<(usize, usize)> {
        match self.compiled.find_from_pos(text, start_byte) {
            Ok(Some(found)) => Some((found.start(), found.end())),
            _ => None,
        }
    }
}

fn utf16_index_to_byte_offset(text: &str, utf16_index: usize) -> usize {
    let mut units_seen = 0;
    for (byte_offset, character) in text.char_indices() {
        if units_seen >= utf16_index {
            return byte_offset;
        }
        units_seen += character.len_utf16();
    }
    text.len()
}

fn count_capture_groups(pattern: &[char]) -> (usize, bool) {
    let mut count = 0;
    let mut has_named_groups = false;
    let mut index = 0;
    let mut inside_class = false;
    while index < pattern.len() {
        match pattern[index] {
            '\\' => index += 1,
            '[' => inside_class = true,
            ']' => inside_class = false,
            '(' if !inside_class => {
                if pattern.get(index + 1) != Some(&'?') {
                    count += 1;
                } else if pattern.get(index + 2) == Some(&'<')
                    && !matches!(pattern.get(index + 3), Some('=') | Some('!'))
                {
                    count += 1;
                    has_named_groups = true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    (count, has_named_groups)
}

fn escape_literal(character: char) -> String {
    if "\\.+*?()|[]{}^$#&-~".contains(character) {
        return format!("\\{character}");
    }
    if character.is_ascii() && !character.is_ascii_graphic() {
        return format!("\\x{{{:X}}}", character as u32);
    }
    character.to_string()
}

fn hex_value(characters: &[char]) -> Option<u32> {
    let text: String = characters.iter().collect();
    if text.is_empty() || !text.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(&text, 16).ok()
}

struct Translator<'pattern> {
    pattern: &'pattern [char],
    flags: JsRegexFlags,
    capture_groups: usize,
    has_named_groups: bool,
    position: usize,
    output: String,
}

enum EscapeTranslation {
    Literal(char),
    ClassItems(&'static str),
    NegatedClass(&'static str),
    Raw(String),
}

pub fn translate(pattern: &str, flags: JsRegexFlags) -> Result<String, JsRegexSyntaxError> {
    let characters: Vec<char> = pattern.chars().collect();
    let (capture_groups, has_named_groups) = count_capture_groups(&characters);
    let mut translator = Translator {
        pattern: &characters,
        flags,
        capture_groups,
        has_named_groups,
        position: 0,
        output: String::new(),
    };
    if flags.ignore_case {
        translator.output.push_str("(?i)");
    }
    translator.translate_sequence()?;
    Ok(translator.output)
}

impl Translator<'_> {
    fn peek(&self, offset: usize) -> Option<char> {
        self.pattern.get(self.position + offset).copied()
    }

    fn translate_sequence(&mut self) -> Result<(), JsRegexSyntaxError> {
        while let Some(character) = self.peek(0) {
            match character {
                '\\' => self.translate_escape_outside_class()?,
                '[' => self.translate_class()?,
                '.' => {
                    self.position += 1;
                    if self.flags.dot_all {
                        self.output.push_str("(?s:.)");
                    } else {
                        self.output
                            .push_str(&format!("[^{LINE_TERMINATOR_CLASS_ITEMS}]"));
                    }
                }
                '^' => {
                    self.position += 1;
                    if self.flags.multiline {
                        self.output
                            .push_str("(?:^|(?<=[\\n\\r\\x{2028}\\x{2029}]))");
                    } else {
                        self.output.push('^');
                    }
                }
                '$' => {
                    self.position += 1;
                    if self.flags.multiline {
                        self.output.push_str("(?:$|(?=[\\n\\r\\x{2028}\\x{2029}]))");
                    } else {
                        self.output.push('$');
                    }
                }
                '{' => {
                    if let Some(length) = self.quantifier_length() {
                        let quantifier: String = self.pattern
                            [self.position..self.position + length]
                            .iter()
                            .collect();
                        self.output.push_str(&quantifier);
                        self.position += length;
                    } else if self.flags.unicode {
                        return Err(JsRegexSyntaxError);
                    } else {
                        self.output.push_str("\\{");
                        self.position += 1;
                    }
                }
                '}' | ']' => {
                    if self.flags.unicode {
                        return Err(JsRegexSyntaxError);
                    }
                    self.output.push('\\');
                    self.output.push(character);
                    self.position += 1;
                }
                '(' => {
                    if self.peek(1) == Some('?')
                        && self.peek(2) == Some('<')
                        && !matches!(self.peek(3), Some('=') | Some('!'))
                    {
                        self.output.push_str("(?P<");
                        self.position += 3;
                    } else {
                        self.output.push('(');
                        self.position += 1;
                    }
                }
                other => {
                    self.output.push_str(&escape_literal_if_needed(other));
                    self.position += 1;
                }
            }
        }
        Ok(())
    }

    fn quantifier_length(&self) -> Option<usize> {
        let mut offset = 1;
        let mut minimum_digits = 0;
        while self
            .peek(offset)
            .is_some_and(|character| character.is_ascii_digit())
        {
            offset += 1;
            minimum_digits += 1;
        }
        if minimum_digits == 0 {
            return None;
        }
        if self.peek(offset) == Some(',') {
            offset += 1;
            while self
                .peek(offset)
                .is_some_and(|character| character.is_ascii_digit())
            {
                offset += 1;
            }
        }
        (self.peek(offset) == Some('}')).then_some(offset + 1)
    }

    fn translate_escape_outside_class(&mut self) -> Result<(), JsRegexSyntaxError> {
        match self.peek(1) {
            Some('b') => {
                self.position += 2;
                self.output.push_str(&format!(
                    "(?:(?<=[{WORD_CLASS_ITEMS}])(?![{WORD_CLASS_ITEMS}])|(?<![{WORD_CLASS_ITEMS}])(?=[{WORD_CLASS_ITEMS}]))"
                ));
                Ok(())
            }
            Some('B') => {
                self.position += 2;
                self.output.push_str(&format!(
                    "(?:(?<=[{WORD_CLASS_ITEMS}])(?=[{WORD_CLASS_ITEMS}])|(?<![{WORD_CLASS_ITEMS}])(?![{WORD_CLASS_ITEMS}]))"
                ));
                Ok(())
            }
            Some(digit) if digit.is_ascii_digit() && digit != '0' => {
                let mut length = 1;
                while self
                    .peek(1 + length)
                    .is_some_and(|character| character.is_ascii_digit())
                {
                    length += 1;
                }
                let digits: String = self.pattern[self.position + 1..self.position + 1 + length]
                    .iter()
                    .collect();
                let group_number: usize = digits.parse().unwrap_or(usize::MAX);
                if group_number <= self.capture_groups {
                    self.position += 1 + length;
                    self.output.push_str(&format!("\\{group_number}"));
                    return Ok(());
                }
                if self.flags.unicode {
                    return Err(JsRegexSyntaxError);
                }
                self.position += 1;
                let literal = self.legacy_octal_escape();
                self.output.push_str(&escape_literal(literal));
                Ok(())
            }
            Some('k') if self.has_named_groups || self.flags.unicode => {
                let name_end = self.pattern[self.position..]
                    .iter()
                    .position(|character| *character == '>');
                match (self.peek(2), name_end) {
                    (Some('<'), Some(end)) => {
                        let name: String = self.pattern[self.position + 3..self.position + end]
                            .iter()
                            .collect();
                        self.position += end + 1;
                        self.output.push_str(&format!("\\k<{name}>"));
                        Ok(())
                    }
                    _ => Err(JsRegexSyntaxError),
                }
            }
            _ => {
                let translation = self.translate_common_escape()?;
                match translation {
                    EscapeTranslation::Literal(character) => {
                        self.output.push_str(&escape_literal(character))
                    }
                    EscapeTranslation::ClassItems(items) => {
                        self.output.push_str(&format!("[{items}]"))
                    }
                    EscapeTranslation::NegatedClass(items) => {
                        self.output.push_str(&format!("[^{items}]"))
                    }
                    EscapeTranslation::Raw(raw) => self.output.push_str(&raw),
                }
                Ok(())
            }
        }
    }

    fn legacy_octal_escape(&mut self) -> char {
        let mut value: u32 = 0;
        let mut length = 0;
        while length < 3 {
            match self.peek(0) {
                Some(digit) if ('0'..='7').contains(&digit) => {
                    let next_value = value * 8 + digit.to_digit(8).unwrap_or(0);
                    if next_value > 0o377 {
                        break;
                    }
                    value = next_value;
                    self.position += 1;
                    length += 1;
                }
                _ => break,
            }
        }
        if length == 0 {
            let literal = self.peek(0).unwrap_or('\\');
            self.position += 1;
            return literal;
        }
        char::from_u32(value).unwrap_or('\u{FFFD}')
    }

    fn translate_common_escape(&mut self) -> Result<EscapeTranslation, JsRegexSyntaxError> {
        let escaped = self.peek(1).ok_or(JsRegexSyntaxError)?;
        self.position += 2;
        Ok(match escaped {
            'd' => EscapeTranslation::ClassItems(DIGIT_CLASS_ITEMS),
            'D' => EscapeTranslation::NegatedClass(DIGIT_CLASS_ITEMS),
            'w' => EscapeTranslation::ClassItems(WORD_CLASS_ITEMS),
            'W' => EscapeTranslation::NegatedClass(WORD_CLASS_ITEMS),
            's' => EscapeTranslation::ClassItems(WHITESPACE_CLASS_ITEMS),
            'S' => EscapeTranslation::NegatedClass(WHITESPACE_CLASS_ITEMS),
            't' => EscapeTranslation::Literal('\t'),
            'n' => EscapeTranslation::Literal('\n'),
            'r' => EscapeTranslation::Literal('\r'),
            'v' => EscapeTranslation::Literal('\u{0B}'),
            'f' => EscapeTranslation::Literal('\u{0C}'),
            '0' if !self
                .peek(0)
                .is_some_and(|character| character.is_ascii_digit()) =>
            {
                EscapeTranslation::Literal('\0')
            }
            '0' => {
                self.position -= 1;
                EscapeTranslation::Literal(self.legacy_octal_escape())
            }
            'c' => match self.peek(0) {
                Some(letter) if letter.is_ascii_alphabetic() => {
                    self.position += 1;
                    EscapeTranslation::Literal(char::from_u32(letter as u32 % 32).unwrap_or('\0'))
                }
                _ => {
                    if self.flags.unicode {
                        return Err(JsRegexSyntaxError);
                    }
                    self.position -= 1;
                    EscapeTranslation::Literal('\\')
                }
            },
            'x' => match hex_value(
                &self.pattern[self.position..(self.position + 2).min(self.pattern.len())],
            ) {
                Some(value) if self.pattern.len() >= self.position + 2 => {
                    self.position += 2;
                    EscapeTranslation::Literal(char::from_u32(value).unwrap_or('\u{FFFD}'))
                }
                _ => {
                    if self.flags.unicode {
                        return Err(JsRegexSyntaxError);
                    }
                    EscapeTranslation::Literal('x')
                }
            },
            'u' => self.translate_unicode_escape()?,
            'p' | 'P' if self.flags.unicode => {
                let close = self.pattern[self.position..]
                    .iter()
                    .position(|character| *character == '}')
                    .ok_or(JsRegexSyntaxError)?;
                if self.peek(0) != Some('{') {
                    return Err(JsRegexSyntaxError);
                }
                let property: String = self.pattern[self.position..self.position + close + 1]
                    .iter()
                    .collect();
                self.position += close + 1;
                EscapeTranslation::Raw(format!("\\{escaped}{property}"))
            }
            other => {
                if self.flags.unicode && other.is_ascii_alphanumeric() {
                    return Err(JsRegexSyntaxError);
                }
                EscapeTranslation::Literal(other)
            }
        })
    }

    fn translate_unicode_escape(&mut self) -> Result<EscapeTranslation, JsRegexSyntaxError> {
        if self.flags.unicode && self.peek(0) == Some('{') {
            let close = self.pattern[self.position..]
                .iter()
                .position(|character| *character == '}')
                .ok_or(JsRegexSyntaxError)?;
            let value = hex_value(&self.pattern[self.position + 1..self.position + close])
                .ok_or(JsRegexSyntaxError)?;
            self.position += close + 1;
            return char::from_u32(value)
                .map(EscapeTranslation::Literal)
                .ok_or(JsRegexSyntaxError);
        }
        let end = (self.position + 4).min(self.pattern.len());
        let Some(value) =
            hex_value(&self.pattern[self.position..end]).filter(|_| end == self.position + 4)
        else {
            if self.flags.unicode {
                return Err(JsRegexSyntaxError);
            }
            return Ok(EscapeTranslation::Literal('u'));
        };
        self.position += 4;
        if (0xD800..0xDC00).contains(&value)
            && self.peek(0) == Some('\\')
            && self.peek(1) == Some('u')
            && let Some(low) = hex_value(
                &self.pattern[(self.position + 2).min(self.pattern.len())
                    ..(self.position + 6).min(self.pattern.len())],
            )
            && (0xDC00..0xE000).contains(&low)
            && self.position + 6 <= self.pattern.len()
        {
            self.position += 6;
            let combined = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
            return Ok(EscapeTranslation::Literal(
                char::from_u32(combined).unwrap_or('\u{FFFD}'),
            ));
        }
        Ok(EscapeTranslation::Literal(
            char::from_u32(value).unwrap_or('\u{FFFD}'),
        ))
    }

    fn translate_class(&mut self) -> Result<(), JsRegexSyntaxError> {
        self.position += 1;
        let negated = self.peek(0) == Some('^');
        if negated {
            self.position += 1;
        }
        if self.peek(0) == Some(']') {
            self.position += 1;
            self.output.push_str(if negated {
                "(?s:.)"
            } else {
                NEVER_MATCHING_CLASS
            });
            return Ok(());
        }
        let mut class = String::from(if negated { "[^" } else { "[" });
        let mut previous_was_class_escape = false;
        loop {
            let character = self.peek(0).ok_or(JsRegexSyntaxError)?;
            match character {
                ']' => {
                    self.position += 1;
                    break;
                }
                '\\' => {
                    if self.peek(1) == Some('b') {
                        self.position += 2;
                        class.push_str("\\x08");
                        previous_was_class_escape = false;
                        continue;
                    }
                    if self.peek(1) == Some('-') {
                        self.position += 2;
                        class.push_str("\\-");
                        previous_was_class_escape = false;
                        continue;
                    }
                    if self.peek(1).is_some_and(|digit| digit.is_ascii_digit())
                        && !self.flags.unicode
                    {
                        self.position += 1;
                        let literal = self.legacy_octal_escape();
                        class.push_str(&escape_literal(literal));
                        previous_was_class_escape = false;
                        continue;
                    }
                    match self.translate_common_escape()? {
                        EscapeTranslation::Literal(literal) => {
                            class.push_str(&escape_literal(literal));
                            previous_was_class_escape = false;
                        }
                        EscapeTranslation::ClassItems(items) => {
                            class.push_str(items);
                            previous_was_class_escape = true;
                        }
                        EscapeTranslation::NegatedClass(items) => {
                            class.push_str(&format!("[^{items}]"));
                            previous_was_class_escape = true;
                        }
                        EscapeTranslation::Raw(raw) => {
                            class.push_str(&raw);
                            previous_was_class_escape = true;
                        }
                    }
                }
                '-' => {
                    self.position += 1;
                    let next_is_class_escape = self.peek(0) == Some('\\')
                        && self
                            .peek(1)
                            .is_some_and(|escaped| "dDwWsS".contains(escaped));
                    if previous_was_class_escape
                        || next_is_class_escape
                        || self.peek(0) == Some('-')
                    {
                        class.push_str("\\-");
                    } else {
                        class.push('-');
                    }
                    previous_was_class_escape = false;
                }
                '[' | '&' | '~' => {
                    self.position += 1;
                    class.push('\\');
                    class.push(character);
                    previous_was_class_escape = false;
                }
                other => {
                    self.position += 1;
                    class.push(other);
                    previous_was_class_escape = false;
                }
            }
        }
        class.push(']');
        self.output.push_str(&class);
        Ok(())
    }
}

fn escape_literal_if_needed(character: char) -> String {
    match character {
        '*' | '+' | '?' | '|' | ')' => character.to_string(),
        '#' | '&' | '~' | '-' | ' ' => escape_literal(character),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, flags: &str, text: &str) -> bool {
        JsRegex::new(pattern, flags)
            .expect("pattern compiles")
            .test(text)
    }

    #[test]
    fn default_case_insensitive_flag_matches_any_case() {
        assert!(matches("feat|fix", "i", "FEAT: login"));
        assert!(!matches("feat|fix", "", "FEAT: login"));
    }

    #[test]
    fn word_and_digit_classes_are_ascii_only_like_javascript() {
        assert!(!matches("^\\w+$", "", "café"));
        assert!(!matches("^\\d$", "", "\u{0663}"));
        assert!(matches("\\bfix\\b", "i", "éfix"));
    }

    #[test]
    fn dot_does_not_match_line_terminators_without_dot_all_flag() {
        assert!(!matches("^a.b$", "", "a\rb"));
        assert!(matches("^a.b$", "s", "a\rb"));
    }

    #[test]
    fn javascript_only_syntax_is_translated() {
        assert!(matches("a{,2}", "", "a{,2}"));
        assert!(matches("[\\w-.]+", "", "a-b.c"));
        assert!(matches("[^]", "", "\n"));
        assert!(matches("\\z", "", "z"));
        assert!(matches("\\/docs", "", "/docs"));
        assert!(matches("(?<!\\\\)@author", "", "hi @author"));
        assert!(matches("(a)\\1", "", "aa"));
        assert!(!matches("@#$@#$@#$", "i", "WIP Title"));
    }

    #[test]
    fn invalid_patterns_and_flags_are_rejected() {
        assert!(JsRegex::new("(", "").is_err());
        assert!(JsRegex::new("*a", "").is_err());
        assert!(JsRegex::new("a", "x").is_err());
        assert!(JsRegex::new("a", "gg").is_err());
    }

    #[test]
    fn global_flag_makes_test_stateful_like_last_index() {
        let regex = JsRegex::new("bug", "gi").expect("pattern compiles");
        let results: Vec<bool> = ["bug", "bug", "bug"]
            .iter()
            .map(|text| regex.test(text))
            .collect();
        assert_eq!(results, vec![true, false, true]);
    }

    #[test]
    fn multiline_anchors_accept_carriage_return() {
        assert!(matches("^b$", "m", "a\rb"));
        assert!(!matches("^b$", "", "a\nb"));
    }
}
