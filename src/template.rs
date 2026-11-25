use crate::annotations;
use crate::error::{Error, ErrorKind, Result};
use crate::json_value;
use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc,
};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::rc::Rc;

const KNOWN_HELPERS: [&str; 8] = [
    "helperMissing",
    "blockHelperMissing",
    "each",
    "if",
    "unless",
    "with",
    "log",
    "lookup",
];
const MERGEABLE_HELPERS: [&str; 6] = [
    "breaklines",
    "toUpperCase",
    "formatDate",
    "displaySettings",
    "ifEquals",
    "statusIcon",
];
const MONTH_ABBREVIATIONS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MAXIMUM_DATE_MILLISECONDS: i64 = 8_640_000_000_000_000;
const MAXIMUM_NESTING_DEPTH: usize = 100;

pub fn render(template: &str, data: &Value) -> Result<String> {
    let mut program = parse(template)?;
    WhitespaceControl::default().visit_program(&mut program);
    let root = TemplateValue::Borrowed(data);
    let scope = Scope {
        context: root.clone(),
        depths: Rc::new(DepthNode {
            value: root.clone(),
            parent: None,
        }),
        data: Rc::new(DataFrame {
            entries: vec![("root".to_string(), root)],
            parent: None,
        }),
        block_params: None,
    };
    render_program(&program, &scope)
}

pub fn compile_and_render(template: &Value, data: &Value) -> Result<String> {
    match template {
        Value::String(text) => render(text, data),
        other => Err(Error::general(format!(
            "You must pass a string or Handlebars AST to Handlebars.compile. You passed {}",
            json_value::to_display_string(Some(other))
        ))),
    }
}

#[must_use]
pub fn template_data(payload: Option<&Value>, validation_result: &Value) -> Value {
    let mut data = Value::Object(Map::new());
    if let Some(payload_value) = payload {
        json_value::deep_merge_into(&mut data, payload_value);
    }
    json_value::deep_merge_into(&mut data, validation_result);
    data
}

pub fn populate_template(
    template: &Value,
    validation_result: &Value,
    payload: Option<&Value>,
    event: &crate::event_aware::EventAttributes<'_>,
) -> Result<String> {
    let data = template_data(payload, validation_result);
    populate_template_with_data(template, &data, payload, event)
}

pub fn populate_template_with_data(
    template: &Value,
    data: &Value,
    payload: Option<&Value>,
    event: &crate::event_aware::EventAttributes<'_>,
) -> Result<String> {
    let template_text = match template {
        Value::String(text) => text,
        Value::Null => return Err(Error::missing_property("replace")),
        _ => {
            json_value::read(json_value::read(payload, "user")?, "login")?;
            return Err(Error::type_mismatch(
                "newTemplate.replace is not a function",
            ));
        }
    };
    let annotated =
        annotations::search_and_replace_special_annotations(template_text, payload, event)?;
    render(&annotated, data)
}

fn is_trimmable_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn is_identifier_character(character: char) -> bool {
    !(is_trimmable_whitespace(character)
        || matches!(character, '!' | '"' | '#' | '%'..=',' | '.' | '/' | ';'..='>' | '@' | '['..='^' | '\x60' | '{'..='~'))
}

fn is_identifier_lookahead(character: Option<char>) -> bool {
    character.is_some_and(|next| {
        matches!(next, '=' | '~' | '}' | '/' | '.' | ')' | '|') || is_trimmable_whitespace(next)
    })
}

fn is_literal_lookahead(character: Option<char>) -> bool {
    character.is_some_and(|next| matches!(next, '~' | '}' | ')') || is_trimmable_whitespace(next))
}

fn leading_whitespace_length(text: &str) -> usize {
    text.len() - text.trim_start_matches(is_trimmable_whitespace).len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Content,
    Comment,
    Open,
    OpenUnescaped,
    OpenBlock,
    OpenEndBlock,
    Inverse,
    OpenInverse,
    OpenInverseChain,
    Close,
    CloseUnescaped,
    OpenSubExpression,
    CloseSubExpression,
    Equals,
    Identifier,
    Separator,
    Data,
    StringLiteral,
    NumberLiteral,
    BooleanLiteral,
    MissingValueLiteral,
    NullLiteral,
    OpenBlockParams,
    CloseBlockParams,
    Unsupported,
    Invalid,
    EndOfInput,
}

#[derive(Debug, Clone)]
struct Token<'source> {
    kind: TokenKind,
    text: Cow<'source, str>,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy)]
struct SourceLocation {
    position: usize,
    line: usize,
    column: usize,
}

impl SourceLocation {
    const START: SourceLocation = SourceLocation {
        position: 0,
        line: 1,
        column: 0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LexerState {
    Initial,
    Mustache,
    EscapedMustache,
    LongComment,
}

struct Lexer<'source> {
    source: &'source str,
    position: usize,
    states: Vec<LexerState>,
    tokens: Vec<Token<'source>>,
    last_location: SourceLocation,
}

impl<'source> Lexer<'source> {
    fn new(source: &'source str) -> Self {
        Lexer {
            source,
            position: 0,
            states: vec![LexerState::Initial],
            tokens: Vec::new(),
            last_location: SourceLocation::START,
        }
    }

    fn location(&mut self, position: usize) -> (usize, usize) {
        let resume_from = if position >= self.last_location.position {
            self.last_location
        } else {
            SourceLocation::START
        };
        let advanced = &self.source[resume_from.position..position];
        let (line, column) = match advanced.rfind('\n') {
            Some(last_newline) => (
                resume_from.line + advanced.bytes().filter(|byte| *byte == b'\n').count(),
                advanced[last_newline + 1..].encode_utf16().count(),
            ),
            None => (
                resume_from.line,
                resume_from.column + advanced.encode_utf16().count(),
            ),
        };
        self.last_location = SourceLocation {
            position,
            line,
            column,
        };
        (line, column)
    }

    fn emit(&mut self, kind: TokenKind, text: impl Into<Cow<'source, str>>, start: usize) {
        let (line, column) = self.location(start);
        self.tokens.push(Token {
            kind,
            text: text.into(),
            line,
            column,
        });
    }

    fn emit_and_advance(&mut self, kind: TokenKind, length: usize) {
        let source = self.source;
        let start = self.position;
        self.emit(kind, &source[start..start + length], start);
        self.position = start + length;
    }

    fn pop_state(&mut self) {
        self.states.pop();
    }

    fn tokenize(mut self) -> Result<Vec<Token<'source>>> {
        loop {
            let state = self.states.last().copied().unwrap_or(LexerState::Initial);
            let keep_going = match state {
                LexerState::Initial => self.lex_initial(),
                LexerState::EscapedMustache => self.lex_escaped_mustache()?,
                LexerState::LongComment => self.lex_long_comment()?,
                LexerState::Mustache => self.lex_mustache(),
            };
            if !keep_going {
                return Ok(self.tokens);
            }
        }
    }

    fn lex_initial(&mut self) -> bool {
        let source = self.source;
        let start = self.position;
        let rest = &source[start..];
        if rest.is_empty() {
            self.emit(TokenKind::EndOfInput, "", start);
            return false;
        }
        if let Some(offset) = rest.find("{{") {
            let mut text = &rest[..offset];
            let next_state = if text.ends_with("\\\\") {
                text = &text[..text.len() - 1];
                LexerState::Mustache
            } else if text.ends_with('\\') {
                text = &text[..text.len() - 1];
                LexerState::EscapedMustache
            } else {
                LexerState::Mustache
            };
            self.states.push(next_state);
            if !text.is_empty() {
                self.emit(TokenKind::Content, text, start);
            }
            self.position = start + offset;
        } else {
            self.emit(TokenKind::Content, rest, start);
            self.position = source.len();
        }
        true
    }

    fn lex_escaped_mustache(&mut self) -> Result<bool> {
        let source = self.source;
        let start = self.position;
        let rest = &source[start..];
        let end = rest
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(rest.len()))
            .skip(2)
            .find(|boundary| {
                let tail = &rest[*boundary..];
                tail.is_empty()
                    || tail.starts_with("{{")
                    || tail.starts_with("\\{{")
                    || tail.starts_with("\\\\{{")
            });
        let Some(end) = end else {
            return Err(self.lexical_error(start));
        };
        self.pop_state();
        self.emit(TokenKind::Content, &rest[..end], start);
        self.position = start + end;
        Ok(true)
    }

    fn lex_long_comment(&mut self) -> Result<bool> {
        let source = self.source;
        let start = self.position;
        let rest = &source[start..];
        let mut search_from = 0;
        while let Some(offset) = rest[search_from..].find("--") {
            let dashes = search_from + offset;
            let after = &rest[dashes + 2..];
            let closing_length = if after.starts_with("}}") {
                Some(2)
            } else if after.starts_with("~}}") {
                Some(3)
            } else {
                None
            };
            if let Some(length) = closing_length {
                self.pop_state();
                self.emit_and_advance(TokenKind::Comment, dashes + 2 + length);
                return Ok(true);
            }
            search_from = dashes + 1;
        }
        Err(self.lexical_error(start))
    }

    fn lexical_error(&mut self, position: usize) -> Error {
        let (line, _) = self.location(position);
        Error::general(format!("Lexical error on line {line}. Unrecognized text."))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one match over all mustache token forms keeps the lexer readable as a single table"
    )]
    fn lex_mustache(&mut self) -> bool {
        let source = self.source;
        let start = self.position;
        let rest = &source[start..];
        if rest.is_empty() {
            self.emit(TokenKind::EndOfInput, "", start);
            return false;
        }
        if rest.starts_with('(') {
            self.emit_and_advance(TokenKind::OpenSubExpression, 1);
            return true;
        }
        if rest.starts_with(')') {
            self.emit_and_advance(TokenKind::CloseSubExpression, 1);
            return true;
        }
        if rest.starts_with("{{{{") || rest.starts_with("}}}}") {
            self.emit_and_advance(TokenKind::Unsupported, 4);
            return true;
        }
        if rest.starts_with("{{") {
            self.lex_open(rest);
            return true;
        }
        if rest.starts_with('=') {
            self.emit_and_advance(TokenKind::Equals, 1);
            return true;
        }
        if rest.starts_with("..") {
            self.emit_and_advance(TokenKind::Identifier, 2);
            return true;
        }
        if rest.starts_with('.') && is_identifier_lookahead(rest[1..].chars().next()) {
            self.emit_and_advance(TokenKind::Identifier, 1);
            return true;
        }
        if rest.starts_with('/') || rest.starts_with('.') {
            self.emit_and_advance(TokenKind::Separator, 1);
            return true;
        }
        let whitespace = leading_whitespace_length(rest);
        if whitespace > 0 {
            self.position = start + whitespace;
            return true;
        }
        if let Some(length) = closing_length(rest, true) {
            self.emit_and_advance(TokenKind::CloseUnescaped, length);
            self.pop_state();
            return true;
        }
        if let Some(length) = closing_length(rest, false) {
            self.emit_and_advance(TokenKind::Close, length);
            self.pop_state();
            return true;
        }
        if rest.starts_with('"') || rest.starts_with('\'') {
            self.lex_string(rest);
            return true;
        }
        if rest.starts_with('@') {
            self.emit_and_advance(TokenKind::Data, 1);
            return true;
        }
        for (keyword, kind) in [
            ("true", TokenKind::BooleanLiteral),
            ("false", TokenKind::BooleanLiteral),
            ("undefined", TokenKind::MissingValueLiteral),
            ("null", TokenKind::NullLiteral),
        ] {
            if rest.starts_with(keyword)
                && is_literal_lookahead(rest[keyword.len()..].chars().next())
            {
                self.emit_and_advance(kind, keyword.len());
                return true;
            }
        }
        if let Some(length) = number_length(rest) {
            self.emit_and_advance(TokenKind::NumberLiteral, length);
            return true;
        }
        if let Some(stripped) = rest.strip_prefix("as") {
            let whitespace = leading_whitespace_length(stripped);
            if whitespace > 0 && stripped[whitespace..].starts_with('|') {
                self.emit_and_advance(TokenKind::OpenBlockParams, 2 + whitespace + 1);
                return true;
            }
        }
        if rest.starts_with('|') {
            self.emit_and_advance(TokenKind::CloseBlockParams, 1);
            return true;
        }
        let identifier_length = rest.len() - rest.trim_start_matches(is_identifier_character).len();
        if identifier_length > 0
            && is_identifier_lookahead(rest[identifier_length..].chars().next())
        {
            self.emit_and_advance(TokenKind::Identifier, identifier_length);
            return true;
        }
        if rest.starts_with('[')
            && let Some(length) = bracket_segment_length(rest)
        {
            let text = unescape_bracket_segment(&rest[..length]);
            self.emit(TokenKind::Identifier, text, start);
            self.position = start + length;
            return true;
        }
        let length = rest.chars().next().map_or(1, char::len_utf8);
        self.emit_and_advance(TokenKind::Invalid, length);
        true
    }

    fn lex_open(&mut self, rest: &'source str) {
        let tilde_length = usize::from(rest[2..].starts_with('~'));
        let prefix_length = 2 + tilde_length;
        let body = &rest[prefix_length..];
        if body.starts_with('>') || body.starts_with("#>") || body.starts_with("#*") {
            self.emit_and_advance(TokenKind::Unsupported, prefix_length + 1);
            return;
        }
        if body.starts_with('#') {
            self.emit_and_advance(TokenKind::OpenBlock, prefix_length + 1);
            return;
        }
        if body.starts_with('/') {
            self.emit_and_advance(TokenKind::OpenEndBlock, prefix_length + 1);
            return;
        }
        if let Some(after_caret) = body.strip_prefix('^') {
            let whitespace = leading_whitespace_length(after_caret);
            if let Some(length) = closing_length(&after_caret[whitespace..], false) {
                self.emit_and_advance(TokenKind::Inverse, prefix_length + 1 + whitespace + length);
                self.pop_state();
                return;
            }
        }
        let whitespace_before_else = leading_whitespace_length(body);
        let after_whitespace = &body[whitespace_before_else..];
        if let Some(after_else) = after_whitespace.strip_prefix("else") {
            let whitespace_after_else = leading_whitespace_length(after_else);
            if let Some(length) = closing_length(&after_else[whitespace_after_else..], false) {
                self.emit_and_advance(
                    TokenKind::Inverse,
                    prefix_length + whitespace_before_else + 4 + whitespace_after_else + length,
                );
                self.pop_state();
                return;
            }
        }
        if body.starts_with('^') {
            self.emit_and_advance(TokenKind::OpenInverse, prefix_length + 1);
            return;
        }
        if let Some(after_else) = after_whitespace.strip_prefix("else") {
            let is_word_boundary = !after_else
                .chars()
                .next()
                .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_');
            if is_word_boundary {
                self.emit_and_advance(
                    TokenKind::OpenInverseChain,
                    prefix_length + whitespace_before_else + 4,
                );
                return;
            }
        }
        if body.starts_with('{') {
            self.emit_and_advance(TokenKind::OpenUnescaped, prefix_length + 1);
            return;
        }
        if body.starts_with('&') {
            self.emit_and_advance(TokenKind::Open, prefix_length + 1);
            return;
        }
        if body.starts_with("!--") {
            self.pop_state();
            self.states.push(LexerState::LongComment);
            return;
        }
        if body.starts_with('!')
            && let Some(offset) = body.find("}}")
        {
            self.emit_and_advance(TokenKind::Comment, prefix_length + offset + 2);
            self.pop_state();
            return;
        }
        if body.starts_with('*') {
            self.emit_and_advance(TokenKind::Unsupported, prefix_length + 1);
            return;
        }
        self.emit_and_advance(TokenKind::Open, prefix_length);
    }

    fn lex_string(&mut self, rest: &'source str) {
        let (quote, escaped_quote, plain_quote) = if rest.starts_with('"') {
            ('"', "\\\"", "\"")
        } else {
            ('\'', "\\'", "'")
        };
        let mut characters = rest.char_indices().skip(1).peekable();
        while let Some((index, character)) = characters.next() {
            if character == '\\' && characters.peek().is_some_and(|(_, next)| *next == quote) {
                characters.next();
                continue;
            }
            if character == quote {
                let inner = &rest[1..index];
                let text = if inner.contains(escaped_quote) {
                    Cow::Owned(inner.replace(escaped_quote, plain_quote))
                } else {
                    Cow::Borrowed(inner)
                };
                let start = self.position;
                self.emit(TokenKind::StringLiteral, text, start);
                self.position = start + index + 1;
                return;
            }
        }
        self.emit_and_advance(TokenKind::Invalid, 1);
    }
}

fn closing_length(text: &str, unescaped: bool) -> Option<usize> {
    let (prefix, remainder) = if unescaped {
        (1, text.strip_prefix('}')?)
    } else {
        (0, text)
    };
    let tilde = usize::from(remainder.starts_with('~'));
    remainder[tilde..]
        .starts_with("}}")
        .then_some(prefix + tilde + 2)
}

fn number_length(text: &str) -> Option<usize> {
    let sign = usize::from(text.starts_with('-'));
    let digits = text[sign..].len()
        - text[sign..]
            .trim_start_matches(|character: char| character.is_ascii_digit())
            .len();
    if digits == 0 {
        return None;
    }
    let integer_end = sign + digits;
    if let Some(after_dot) = text[integer_end..].strip_prefix('.') {
        let fraction = after_dot.len()
            - after_dot
                .trim_start_matches(|character: char| character.is_ascii_digit())
                .len();
        let end = integer_end + 1 + fraction;
        if fraction > 0 && is_literal_lookahead(text[end..].chars().next()) {
            return Some(end);
        }
    }
    is_literal_lookahead(text[integer_end..].chars().next()).then_some(integer_end)
}

fn bracket_segment_length(text: &str) -> Option<usize> {
    let mut characters = text.char_indices().skip(1).peekable();
    while let Some((index, character)) = characters.next() {
        if character == '\\' && characters.peek().is_some_and(|(_, next)| *next == ']') {
            characters.next();
            continue;
        }
        if character == ']' {
            return Some(index + 1);
        }
    }
    None
}

fn unescape_bracket_segment(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\'
            && let Some(next) = characters
                .peek()
                .copied()
                .filter(|next| *next == '\\' || *next == ']')
        {
            unescaped.push(next);
            characters.next();
            continue;
        }
        unescaped.push(character);
    }
    unescaped
}

fn segment_identifier(token_text: &str) -> String {
    if token_text.len() >= 2 && token_text.starts_with('[') && token_text.ends_with(']') {
        token_text[1..token_text.len() - 1].to_string()
    } else {
        token_text.to_string()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Strip {
    open: bool,
    close: bool,
}

fn strip_flags(open: &str, close: &str) -> Strip {
    Strip {
        open: open.chars().nth(2) == Some('~'),
        close: close.chars().rev().nth(2) == Some('~'),
    }
}

#[derive(Debug, Clone)]
struct PathExpression {
    data: bool,
    depth: usize,
    parts: Vec<String>,
    source_text: String,
}

#[derive(Debug, Clone)]
enum Expression {
    Path(PathExpression),
    Invocation(Box<Call>),
    StringLiteral(String),
    NumberLiteral(f64),
    BooleanLiteral(bool),
    MissingValueLiteral,
    NullLiteral,
}

#[derive(Debug, Clone)]
struct Call {
    path: Expression,
    params: Vec<Expression>,
    hash: Option<Vec<(String, Expression)>>,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone)]
struct ContentNode {
    source_text: String,
    value: String,
    right_stripped: bool,
    left_stripped: bool,
}

#[derive(Debug, Clone, Copy)]
enum OutputEscaping {
    Html,
    Raw,
}

#[derive(Debug, Clone)]
struct Mustache {
    call: Call,
    escaping: OutputEscaping,
    strip: Strip,
}

#[derive(Debug, Clone, Default)]
struct Program {
    body: Vec<Statement>,
    block_params: Option<Vec<String>>,
    chained: bool,
}

#[derive(Debug, Clone)]
struct Block {
    call: Call,
    program: Option<Program>,
    inverse: Option<Program>,
    open_strip: Strip,
    inverse_strip: Option<Strip>,
    close_strip: Option<Strip>,
}

#[derive(Debug, Clone)]
enum Statement {
    Content(ContentNode),
    Comment(Strip),
    Mustache(Mustache),
    Block(Box<Block>),
}

struct OpenBlock {
    call: Call,
    block_params: Option<Vec<String>>,
    strip: Strip,
}

struct InverseAndProgram {
    strip: Strip,
    program: Program,
    chain: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockOpening {
    Regular,
    Inverted,
}

enum BlockClose {
    Path { source_text: String, strip: Strip },
    Inverse(Strip),
    Missing,
}

fn literal_source_text(expression: &Expression) -> String {
    match expression {
        Expression::Path(path) => path.source_text.clone(),
        Expression::Invocation(_) => String::new(),
        Expression::StringLiteral(text) => text.clone(),
        Expression::NumberLiteral(number) => json_value::float_to_string(*number),
        Expression::BooleanLiteral(flag) => flag.to_string(),
        Expression::MissingValueLiteral => "undefined".to_string(),
        Expression::NullLiteral => "null".to_string(),
    }
}

fn parse(template: &str) -> Result<Program> {
    let tokens = Lexer::new(template).tokenize()?;
    let mut parser = Parser {
        tokens,
        index: 0,
        nesting_depth: 0,
    };
    let program = parser.parse_program()?;
    if parser.peek() != TokenKind::EndOfInput {
        return Err(parser.error());
    }
    Ok(program)
}

struct Parser<'source> {
    tokens: Vec<Token<'source>>,
    index: usize,
    nesting_depth: usize,
}

impl<'source> Parser<'source> {
    fn peek(&self) -> TokenKind {
        self.peek_at(0)
    }

    fn peek_at(&self, offset: usize) -> TokenKind {
        self.tokens
            .get(self.index + offset)
            .map_or(TokenKind::EndOfInput, |token| token.kind)
    }

    fn current_location(&self) -> (usize, usize) {
        self.tokens
            .get(self.index)
            .map_or((1, 0), |token| (token.line, token.column))
    }

    fn advance(&mut self) -> Token<'source> {
        let token = match self.tokens.get_mut(self.index) {
            Some(token) => Token {
                kind: token.kind,
                text: std::mem::take(&mut token.text),
                line: token.line,
                column: token.column,
            },
            None => Token {
                kind: TokenKind::EndOfInput,
                text: Cow::Borrowed(""),
                line: 1,
                column: 0,
            },
        };
        self.index += 1;
        token
    }

    fn error(&self) -> Error {
        let token = self.tokens.get(self.index);
        let line = token.map_or(1, |token| token.line);
        let found = token.map_or(TokenKind::EndOfInput, |token| token.kind);
        Error::general(format!("Parse error on line {line}: unexpected {found:?}"))
    }

    fn nested<Parsed>(
        &mut self,
        parse: impl FnOnce(&mut Self) -> Result<Parsed>,
    ) -> Result<Parsed> {
        if self.nesting_depth >= MAXIMUM_NESTING_DEPTH {
            return Err(Error::new(
                ErrorKind::OutOfRange,
                "Maximum call stack size exceeded",
            ));
        }
        self.nesting_depth += 1;
        let parsed = parse(self);
        self.nesting_depth -= 1;
        parsed
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token<'source>> {
        if self.peek() == kind {
            Ok(self.advance())
        } else {
            Err(self.error())
        }
    }

    fn parse_program(&mut self) -> Result<Program> {
        let mut body = Vec::new();
        loop {
            match self.peek() {
                TokenKind::Content => {
                    let text = self.advance().text.into_owned();
                    body.push(Statement::Content(ContentNode {
                        source_text: text.clone(),
                        value: text,
                        right_stripped: false,
                        left_stripped: false,
                    }));
                }
                TokenKind::Comment => {
                    let token = self.advance();
                    body.push(Statement::Comment(strip_flags(&token.text, &token.text)));
                }
                TokenKind::Open | TokenKind::OpenUnescaped => {
                    body.push(Statement::Mustache(self.parse_mustache()?));
                }
                TokenKind::OpenBlock | TokenKind::OpenInverse => {
                    body.push(Statement::Block(Box::new(self.nested(Self::parse_block)?)));
                }
                TokenKind::Unsupported | TokenKind::Invalid => return Err(self.error()),
                _ => break,
            }
        }
        Ok(Program {
            body,
            block_params: None,
            chained: false,
        })
    }

    fn parse_mustache(&mut self) -> Result<Mustache> {
        let open = self.advance();
        let call = self.parse_call()?;
        let close = if open.kind == TokenKind::OpenUnescaped {
            self.expect(TokenKind::CloseUnescaped)?
        } else {
            self.expect(TokenKind::Close)?
        };
        let escape_flag = open
            .text
            .chars()
            .nth(3)
            .or_else(|| open.text.chars().nth(2));
        Ok(Mustache {
            call,
            escaping: if matches!(escape_flag, Some('{' | '&')) {
                OutputEscaping::Raw
            } else {
                OutputEscaping::Html
            },
            strip: strip_flags(&open.text, &close.text),
        })
    }

    fn parse_call(&mut self) -> Result<Call> {
        let (line, column) = self.current_location();
        let path = self.parse_helper_name()?;
        let mut params = Vec::new();
        while is_param_start(self.peek())
            && !(self.peek() == TokenKind::Identifier && self.peek_at(1) == TokenKind::Equals)
        {
            params.push(self.parse_param()?);
        }
        let mut pairs = Vec::new();
        while self.peek() == TokenKind::Identifier && self.peek_at(1) == TokenKind::Equals {
            let key = self.advance();
            self.advance();
            pairs.push((segment_identifier(&key.text), self.parse_param()?));
        }
        Ok(Call {
            path,
            params,
            hash: (!pairs.is_empty()).then_some(pairs),
            line,
            column,
        })
    }

    fn parse_param(&mut self) -> Result<Expression> {
        if self.peek() == TokenKind::OpenSubExpression {
            self.advance();
            let call = self.nested(Self::parse_call)?;
            self.expect(TokenKind::CloseSubExpression)?;
            return Ok(Expression::Invocation(Box::new(call)));
        }
        self.parse_helper_name()
    }

    fn parse_helper_name(&mut self) -> Result<Expression> {
        match self.peek() {
            TokenKind::Identifier => Ok(Expression::Path(self.parse_path(false)?)),
            TokenKind::Data => {
                self.advance();
                Ok(Expression::Path(self.parse_path(true)?))
            }
            TokenKind::StringLiteral => {
                Ok(Expression::StringLiteral(self.advance().text.into_owned()))
            }
            TokenKind::NumberLiteral => {
                let text = self.advance().text;
                Ok(Expression::NumberLiteral(
                    text.parse::<f64>().unwrap_or(f64::NAN),
                ))
            }
            TokenKind::BooleanLiteral => {
                Ok(Expression::BooleanLiteral(self.advance().text == "true"))
            }
            TokenKind::MissingValueLiteral => {
                self.advance();
                Ok(Expression::MissingValueLiteral)
            }
            TokenKind::NullLiteral => {
                self.advance();
                Ok(Expression::NullLiteral)
            }
            _ => Err(self.error()),
        }
    }

    fn parse_path(&mut self, data: bool) -> Result<PathExpression> {
        let (line, column) = self.current_location();
        let first = self.expect(TokenKind::Identifier)?;
        let mut segments = vec![(
            segment_identifier(&first.text),
            first.text,
            Cow::Borrowed(""),
        )];
        while self.peek() == TokenKind::Separator {
            let separator = self.advance();
            let identifier = self.expect(TokenKind::Identifier)?;
            segments.push((
                segment_identifier(&identifier.text),
                identifier.text,
                separator.text,
            ));
        }
        let mut source_text = if data { "@".to_string() } else { String::new() };
        let mut parts = Vec::new();
        let mut depth = 0;
        for (part, token_text, separator) in segments {
            let is_literal = token_text.as_ref() != part.as_str();
            source_text.push_str(&separator);
            source_text.push_str(&part);
            if !is_literal && (part == ".." || part == "." || part == "this") {
                if !parts.is_empty() {
                    return Err(Error::general(format!(
                        "Invalid path: {source_text} - {line}:{column}"
                    )));
                }
                if part == ".." {
                    depth += 1;
                }
            } else {
                parts.push(part);
            }
        }
        Ok(PathExpression {
            data,
            depth,
            parts,
            source_text,
        })
    }

    fn parse_open_block(&mut self, open: &Token<'source>) -> Result<OpenBlock> {
        let call = self.parse_call()?;
        let block_params = if self.peek() == TokenKind::OpenBlockParams {
            self.advance();
            let mut names = Vec::new();
            while self.peek() == TokenKind::Identifier {
                names.push(segment_identifier(&self.advance().text));
            }
            if names.is_empty() {
                return Err(self.error());
            }
            self.expect(TokenKind::CloseBlockParams)?;
            Some(names)
        } else {
            None
        };
        let close = self.expect(TokenKind::Close)?;
        Ok(OpenBlock {
            call,
            block_params,
            strip: strip_flags(&open.text, &close.text),
        })
    }

    fn parse_block(&mut self) -> Result<Block> {
        let open_token = self.advance();
        let opening = if open_token.kind == TokenKind::OpenInverse {
            BlockOpening::Inverted
        } else {
            BlockOpening::Regular
        };
        let open = self.parse_open_block(&open_token)?;
        let program = self.parse_program()?;
        let inverse_and_program = match self.peek() {
            TokenKind::OpenInverseChain if opening == BlockOpening::Regular => {
                Some(self.nested(Self::parse_inverse_chain)?)
            }
            TokenKind::Inverse => Some(self.parse_inverse_and_program()?),
            _ => None,
        };
        let close = self.parse_close_block()?;
        prepare_block(open, program, inverse_and_program, &close, opening)
    }

    fn parse_close_block(&mut self) -> Result<BlockClose> {
        let open = self.expect(TokenKind::OpenEndBlock)?;
        let path = self.parse_helper_name()?;
        let close = self.expect(TokenKind::Close)?;
        Ok(BlockClose::Path {
            source_text: literal_source_text(&path),
            strip: strip_flags(&open.text, &close.text),
        })
    }

    fn parse_inverse_and_program(&mut self) -> Result<InverseAndProgram> {
        let token = self.advance();
        let program = self.parse_program()?;
        Ok(InverseAndProgram {
            strip: strip_flags(&token.text, &token.text),
            program,
            chain: false,
        })
    }

    fn parse_inverse_chain(&mut self) -> Result<InverseAndProgram> {
        let open_token = self.advance();
        let open = self.parse_open_block(&open_token)?;
        let program = self.parse_program()?;
        let nested = match self.peek() {
            TokenKind::OpenInverseChain => Some(self.nested(Self::parse_inverse_chain)?),
            TokenKind::Inverse => Some(self.parse_inverse_and_program()?),
            _ => None,
        };
        let close = match &nested {
            Some(inner) => BlockClose::Inverse(inner.strip),
            None => BlockClose::Missing,
        };
        let strip = open.strip;
        let block = prepare_block(open, program, nested, &close, BlockOpening::Regular)?;
        Ok(InverseAndProgram {
            strip,
            program: Program {
                body: vec![Statement::Block(Box::new(block))],
                block_params: None,
                chained: true,
            },
            chain: true,
        })
    }
}

fn is_param_start(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Identifier
            | TokenKind::Data
            | TokenKind::StringLiteral
            | TokenKind::NumberLiteral
            | TokenKind::BooleanLiteral
            | TokenKind::MissingValueLiteral
            | TokenKind::NullLiteral
            | TokenKind::OpenSubExpression
    )
}

fn prepare_block(
    open: OpenBlock,
    program: Program,
    inverse_and_program: Option<InverseAndProgram>,
    close: &BlockClose,
    opening: BlockOpening,
) -> Result<Block> {
    if let BlockClose::Path { source_text, .. } = close {
        let open_source_text = literal_source_text(&open.call.path);
        if &open_source_text != source_text {
            return Err(Error::general(format!(
                "{open_source_text} doesn't match {source_text} - {}:{}",
                open.call.line, open.call.column
            )));
        }
    }
    let mut program = program;
    program.block_params = open.block_params;
    let close_strip = match close {
        BlockClose::Path { strip, .. } | BlockClose::Inverse(strip) => Some(*strip),
        BlockClose::Missing => None,
    };
    let mut inverse = None;
    let mut inverse_strip = None;
    if let Some(mut inverse_part) = inverse_and_program {
        if inverse_part.chain
            && let Some(Statement::Block(first)) = inverse_part.program.body.first_mut()
        {
            first.close_strip = close_strip;
        }
        inverse_strip = Some(inverse_part.strip);
        inverse = Some(inverse_part.program);
    }
    let (program, inverse) = match opening {
        BlockOpening::Inverted => (inverse, Some(program)),
        BlockOpening::Regular => (Some(program), inverse),
    };
    Ok(Block {
        call: open.call,
        program,
        inverse,
        open_strip: open.strip,
        inverse_strip,
        close_strip,
    })
}

#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the whitespace control pass tracks independent strip and standalone flags"
)]
struct StandaloneStrip {
    open: bool,
    close: bool,
    open_standalone: bool,
    close_standalone: bool,
    inline_standalone: bool,
}

#[derive(Default)]
struct WhitespaceControl {
    root_seen: bool,
}

fn trailing_whitespace_has_newline(text: &str, whole_whitespace_counts: bool) -> bool {
    let trimmed = text.trim_end_matches(is_trimmable_whitespace);
    text[trimmed.len()..].contains('\n') || (whole_whitespace_counts && trimmed.is_empty())
}

fn leading_whitespace_has_newline(text: &str, whole_whitespace_counts: bool) -> bool {
    let trimmed = text.trim_start_matches(is_trimmable_whitespace);
    text[..text.len() - trimmed.len()].contains('\n')
        || (whole_whitespace_counts && trimmed.is_empty())
}

fn is_previous_whitespace(body: &[Statement], index: Option<usize>, is_root: bool) -> bool {
    let index = index.unwrap_or(body.len());
    let Some(previous) = index.checked_sub(1).and_then(|position| body.get(position)) else {
        return is_root;
    };
    let has_sibling = index >= 2;
    match previous {
        Statement::Content(content) => {
            trailing_whitespace_has_newline(&content.source_text, !has_sibling && is_root)
        }
        _ => false,
    }
}

fn is_next_whitespace(body: &[Statement], index: Option<usize>, is_root: bool) -> bool {
    let next_index = index.map_or(0, |position| position + 1);
    let Some(next) = body.get(next_index) else {
        return is_root;
    };
    let has_sibling = body.get(next_index + 1).is_some();
    match next {
        Statement::Content(content) => {
            leading_whitespace_has_newline(&content.source_text, !has_sibling && is_root)
        }
        _ => false,
    }
}

fn strip_line_start(text: &str) -> &str {
    let without_blanks = text.trim_start_matches([' ', '\t']);
    let without_carriage_return = without_blanks
        .strip_prefix('\r')
        .filter(|rest| rest.starts_with('\n'))
        .unwrap_or(without_blanks);
    without_carriage_return
        .strip_prefix('\n')
        .unwrap_or(without_carriage_return)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WhitespaceExtent {
    CurrentLine,
    All,
}

fn omit_right(body: &mut [Statement], index: Option<usize>, extent: WhitespaceExtent) {
    let position = index.map_or(0, |value| value + 1);
    let Some(Statement::Content(current)) = body.get_mut(position) else {
        return;
    };
    if extent == WhitespaceExtent::CurrentLine && current.right_stripped {
        return;
    }
    let kept_length = match extent {
        WhitespaceExtent::All => current
            .value
            .trim_start_matches(is_trimmable_whitespace)
            .len(),
        WhitespaceExtent::CurrentLine => strip_line_start(&current.value).len(),
    };
    let removed_length = current.value.len() - kept_length;
    current.right_stripped = removed_length > 0;
    current.value.drain(..removed_length);
}

fn omit_left(body: &mut [Statement], index: Option<usize>, extent: WhitespaceExtent) -> bool {
    let position = match index {
        None => body.len().checked_sub(1),
        Some(value) => value.checked_sub(1),
    };
    let Some(Statement::Content(current)) = position.and_then(|value| body.get_mut(value)) else {
        return false;
    };
    if extent == WhitespaceExtent::CurrentLine && current.left_stripped {
        return false;
    }
    let kept_length = match extent {
        WhitespaceExtent::All => current
            .value
            .trim_end_matches(is_trimmable_whitespace)
            .len(),
        WhitespaceExtent::CurrentLine => current.value.trim_end_matches([' ', '\t']).len(),
    };
    current.left_stripped = kept_length != current.value.len();
    current.value.truncate(kept_length);
    current.left_stripped
}

fn main_program(block: &mut Block) -> Option<&mut Program> {
    if block.program.is_some() {
        block.program.as_mut()
    } else {
        block.inverse.as_mut()
    }
}

fn closing_program(block: &mut Block) -> Option<&mut Program> {
    if block.inverse.is_some() {
        block.inverse.as_mut()
    } else {
        block.program.as_mut()
    }
}

fn first_inverse(block: &mut Block) -> Option<&mut Program> {
    let inverse = block.inverse.as_mut()?;
    if inverse.chained {
        match inverse.body.first_mut() {
            Some(Statement::Block(chained)) => chained.program.as_mut(),
            _ => None,
        }
    } else {
        Some(inverse)
    }
}

fn last_inverse(block: &mut Block) -> Option<&mut Program> {
    let mut current = block.inverse.as_mut()?;
    while current.chained {
        current = match current.body.last_mut() {
            Some(Statement::Block(chained)) => chained.program.as_mut()?,
            _ => return None,
        };
    }
    Some(current)
}

fn omit_right_of_body(program: Option<&mut Program>, extent: WhitespaceExtent) {
    if let Some(program) = program {
        omit_right(&mut program.body, None, extent);
    }
}

fn omit_left_of_body(program: Option<&mut Program>, extent: WhitespaceExtent) {
    if let Some(program) = program {
        omit_left(&mut program.body, None, extent);
    }
}

fn body_starts_on_new_line(program: Option<&mut Program>) -> bool {
    program.is_some_and(|program| is_next_whitespace(&program.body, None, false))
}

fn body_ends_on_new_line(program: Option<&mut Program>) -> bool {
    program.is_some_and(|program| is_previous_whitespace(&program.body, None, false))
}

impl WhitespaceControl {
    fn visit_program(&mut self, program: &mut Program) {
        let is_root = !self.root_seen;
        self.root_seen = true;
        let body = &mut program.body;
        for index in 0..body.len() {
            let strip = match &mut body[index] {
                Statement::Content(_) => continue,
                Statement::Comment(strip) => StandaloneStrip {
                    open: strip.open,
                    close: strip.close,
                    inline_standalone: true,
                    ..StandaloneStrip::default()
                },
                Statement::Mustache(mustache) => StandaloneStrip {
                    open: mustache.strip.open,
                    close: mustache.strip.close,
                    ..StandaloneStrip::default()
                },
                Statement::Block(block) => self.visit_block(block),
            };
            let previous_whitespace = is_previous_whitespace(body, Some(index), is_root);
            let next_whitespace = is_next_whitespace(body, Some(index), is_root);
            let open_standalone = strip.open_standalone && previous_whitespace;
            let close_standalone = strip.close_standalone && next_whitespace;
            let inline_standalone =
                strip.inline_standalone && previous_whitespace && next_whitespace;
            if strip.close {
                omit_right(body, Some(index), WhitespaceExtent::All);
            }
            if strip.open {
                omit_left(body, Some(index), WhitespaceExtent::All);
            }
            if inline_standalone {
                omit_right(body, Some(index), WhitespaceExtent::CurrentLine);
                omit_left(body, Some(index), WhitespaceExtent::CurrentLine);
            }
            if open_standalone {
                if let Statement::Block(block) = &mut body[index] {
                    omit_right_of_body(main_program(block), WhitespaceExtent::CurrentLine);
                }
                omit_left(body, Some(index), WhitespaceExtent::CurrentLine);
            }
            if close_standalone {
                omit_right(body, Some(index), WhitespaceExtent::CurrentLine);
                if let Statement::Block(block) = &mut body[index] {
                    omit_left_of_body(closing_program(block), WhitespaceExtent::CurrentLine);
                }
            }
        }
    }

    fn visit_block(&mut self, block: &mut Block) -> StandaloneStrip {
        if let Some(program) = block.program.as_mut() {
            self.visit_program(program);
        }
        if let Some(inverse) = block.inverse.as_mut() {
            self.visit_program(inverse);
        }
        let has_inverse = block.program.is_some() && block.inverse.is_some();
        let open_strip = block.open_strip;
        let close_strip = block.close_strip.unwrap_or_default();
        let inverse_strip = block.inverse_strip.unwrap_or_default();
        let open_standalone = body_starts_on_new_line(main_program(block));
        let close_standalone = if has_inverse {
            body_ends_on_new_line(first_inverse(block))
        } else {
            body_ends_on_new_line(main_program(block))
        };
        let strip = StandaloneStrip {
            open: open_strip.open,
            close: close_strip.close,
            open_standalone,
            close_standalone,
            inline_standalone: false,
        };
        if open_strip.close {
            omit_right_of_body(main_program(block), WhitespaceExtent::All);
        }
        if has_inverse {
            if inverse_strip.open {
                omit_left_of_body(main_program(block), WhitespaceExtent::All);
            }
            if inverse_strip.close {
                omit_right_of_body(first_inverse(block), WhitespaceExtent::All);
            }
            if close_strip.open {
                omit_left_of_body(last_inverse(block), WhitespaceExtent::All);
            }
            let program_ends_with_newline = body_ends_on_new_line(main_program(block));
            let inverse_starts_with_newline = body_starts_on_new_line(first_inverse(block));
            if program_ends_with_newline && inverse_starts_with_newline {
                omit_left_of_body(main_program(block), WhitespaceExtent::CurrentLine);
                omit_right_of_body(first_inverse(block), WhitespaceExtent::CurrentLine);
            }
        } else if close_strip.open {
            omit_left_of_body(main_program(block), WhitespaceExtent::All);
        }
        strip
    }
}

#[derive(Clone)]
enum TemplateValue<'data> {
    Missing,
    Borrowed(&'data Value),
    Owned(Rc<Value>),
    Safe(Rc<str>),
    NullContext,
}

impl<'data> TemplateValue<'data> {
    fn owned(value: Value) -> Self {
        TemplateValue::Owned(Rc::new(value))
    }

    fn text(text: impl Into<String>) -> Self {
        TemplateValue::owned(Value::String(text.into()))
    }

    fn value(&self) -> Option<&Value> {
        match self {
            TemplateValue::Borrowed(value) => Some(value),
            TemplateValue::Owned(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    fn is_nullish(&self) -> bool {
        matches!(self, TemplateValue::Missing) || matches!(self.value(), Some(Value::Null))
    }

    fn is_truthy(&self) -> bool {
        match self {
            TemplateValue::Missing => false,
            TemplateValue::Safe(_) | TemplateValue::NullContext => true,
            _ => json_value::truthy(self.value()),
        }
    }

    fn child(&self, select: impl FnOnce(&Value) -> Option<&Value>) -> TemplateValue<'data> {
        match self {
            TemplateValue::Borrowed(value) => {
                select(value).map_or(TemplateValue::Missing, TemplateValue::Borrowed)
            }
            TemplateValue::Owned(value) => select(value)
                .map_or(TemplateValue::Missing, |selected| {
                    TemplateValue::owned(selected.clone())
                }),
            _ => TemplateValue::Missing,
        }
    }
}

fn display_text<'value>(value: &'value TemplateValue) -> Cow<'value, str> {
    match value {
        TemplateValue::Missing => Cow::Borrowed("undefined"),
        TemplateValue::Safe(text) => Cow::Borrowed(text),
        TemplateValue::NullContext => Cow::Borrowed("[object Object]"),
        _ => match value.value() {
            Some(Value::String(text)) => Cow::Borrowed(text),
            other => Cow::Owned(json_value::to_display_string(other)),
        },
    }
}

fn to_display_string(value: &TemplateValue) -> String {
    display_text(value).into_owned()
}

fn push_escaped_html(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            '\x60' => output.push_str("&#x60;"),
            '=' => output.push_str("&#x3D;"),
            other => output.push(other),
        }
    }
}

fn push_escaped_expression(output: &mut String, value: &TemplateValue) {
    match value {
        TemplateValue::Safe(text) => output.push_str(text),
        _ if value.is_nullish() => {}
        _ => push_escaped_html(output, &display_text(value)),
    }
}

fn push_appended_string(output: &mut String, value: &TemplateValue) {
    if !value.is_nullish() {
        output.push_str(&display_text(value));
    }
}

fn array_index(name: &str) -> Option<usize> {
    let index = name
        .parse::<u32>()
        .ok()
        .filter(|index| *index < u32::MAX && index.to_string() == name)?;
    usize::try_from(index).ok()
}

fn keys_in_property_order(map: &Map<String, Value>) -> Vec<&String> {
    let mut index_keys: Vec<(usize, &String)> = map
        .keys()
        .filter_map(|key| array_index(key).map(|index| (index, key)))
        .collect();
    index_keys.sort_by_key(|(index, _)| *index);
    index_keys
        .into_iter()
        .map(|(_, key)| key)
        .chain(map.keys().filter(|key| array_index(key).is_none()))
        .collect()
}

fn lookup_property<'data>(parent: &TemplateValue<'data>, name: &str) -> TemplateValue<'data> {
    match parent.value() {
        Some(Value::Object(_)) => parent.child(|value| value.get(name)),
        Some(Value::Array(items)) => {
            if name == "length" {
                TemplateValue::owned(Value::from(items.len()))
            } else if let Some(index) = array_index(name) {
                parent.child(|value| value.as_array().and_then(|items| items.get(index)))
            } else {
                TemplateValue::Missing
            }
        }
        Some(Value::String(text)) => {
            if name == "length" {
                TemplateValue::owned(Value::from(text.encode_utf16().count()))
            } else if let Some(unit) =
                array_index(name).and_then(|index| text.encode_utf16().nth(index))
            {
                TemplateValue::text(String::from_utf16_lossy(&[unit]))
            } else {
                TemplateValue::Missing
            }
        }
        _ => TemplateValue::Missing,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "integral numbers below 2^53 convert to i64 exactly"
)]
fn number_literal_value(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
        Value::from(number as i64)
    } else {
        serde_json::Number::from_f64(number).map_or(Value::Null, Value::Number)
    }
}

fn string_to_number(text: &str) -> f64 {
    let trimmed = text.trim_matches(is_trimmable_whitespace);
    if trimmed.is_empty() {
        0.0
    } else {
        trimmed.parse::<f64>().unwrap_or(f64::NAN)
    }
}

fn loosely_equal_primitives(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(first), Value::Number(second)) => first.as_f64() == second.as_f64(),
        (Value::String(first), Value::String(second)) => first == second,
        (Value::Bool(first), Value::Bool(second)) => first == second,
        (Value::Bool(flag), other) | (other, Value::Bool(flag)) => {
            loosely_equal_primitives(&Value::from(u8::from(*flag)), other)
        }
        (Value::Number(number), Value::String(text))
        | (Value::String(text), Value::Number(number)) => {
            number.as_f64() == Some(string_to_number(text))
        }
        _ => false,
    }
}

fn is_reference_value(value: &Value) -> bool {
    matches!(value, Value::Array(_) | Value::Object(_))
}

fn same_reference(left: &TemplateValue, right: &TemplateValue) -> bool {
    match (left, right) {
        (TemplateValue::Borrowed(first), TemplateValue::Borrowed(second)) => {
            std::ptr::eq(*first, *second)
        }
        (TemplateValue::Owned(first), TemplateValue::Owned(second)) => Rc::ptr_eq(first, second),
        _ => false,
    }
}

fn loosely_equal(left: &TemplateValue, right: &TemplateValue) -> bool {
    if left.is_nullish() || right.is_nullish() {
        return left.is_nullish() && right.is_nullish();
    }
    match (left, right) {
        (TemplateValue::NullContext, TemplateValue::NullContext) => true,
        (TemplateValue::Safe(first), TemplateValue::Safe(second)) => Rc::ptr_eq(first, second),
        _ => match (left.value(), right.value()) {
            (Some(first), Some(second)) => {
                match (is_reference_value(first), is_reference_value(second)) {
                    (true, true) => same_reference(left, right),
                    (false, false) => loosely_equal_primitives(first, second),
                    (true, false) => loosely_equal_primitives(
                        &Value::String(json_value::to_display_string(Some(first))),
                        second,
                    ),
                    (false, true) => loosely_equal_primitives(
                        first,
                        &Value::String(json_value::to_display_string(Some(second))),
                    ),
                }
            }
            _ => false,
        },
    }
}

fn strictly_equal(left: &TemplateValue, right: &TemplateValue) -> bool {
    match (left, right) {
        (TemplateValue::Missing, TemplateValue::Missing)
        | (TemplateValue::NullContext, TemplateValue::NullContext) => true,
        (TemplateValue::Safe(first), TemplateValue::Safe(second)) => Rc::ptr_eq(first, second),
        _ => match (left.value(), right.value()) {
            (Some(first), Some(second))
                if is_reference_value(first) && is_reference_value(second) =>
            {
                same_reference(left, right)
            }
            (Some(first), Some(second)) => json_value::strict_equals(first, second),
            _ => false,
        },
    }
}

fn is_empty(value: &TemplateValue) -> bool {
    let is_zero =
        matches!(value.value(), Some(Value::Number(number)) if number.as_f64() == Some(0.0));
    if !value.is_truthy() && !is_zero {
        return true;
    }
    matches!(value.value(), Some(Value::Array(items)) if items.is_empty())
}

fn json_stringify(value: &Value) -> String {
    let mut output = String::new();
    push_json(&mut output, value);
    output
}

fn push_json_string(output: &mut String, text: &str) {
    output.push_str(&serde_json::to_string(text).unwrap_or_default());
}

fn push_json(output: &mut String, value: &Value) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(flag) => output.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => output.push_str(&json_value::number_to_string(number)),
        Value::String(text) => push_json_string(output, text),
        Value::Array(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                push_json(output, item);
            }
            output.push(']');
        }
        Value::Object(map) => {
            output.push('{');
            for (index, key) in keys_in_property_order(map).into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                push_json_string(output, key);
                output.push(':');
                if let Some(item) = map.get(key) {
                    push_json(output, item);
                }
            }
            output.push('}');
        }
    }
}

struct DataFrame<'data> {
    entries: Vec<(String, TemplateValue<'data>)>,
    parent: Option<Rc<DataFrame<'data>>>,
}

impl<'data> DataFrame<'data> {
    fn get(&self, name: &str) -> TemplateValue<'data> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map_or(TemplateValue::Missing, |(_, value)| value.clone())
    }

    fn child_frame(
        parent: &Rc<DataFrame<'data>>,
        updates: [(&str, TemplateValue<'data>); 4],
    ) -> Rc<DataFrame<'data>> {
        let mut entries = parent.entries.clone();
        for (name, value) in updates {
            match entries.iter_mut().find(|(key, _)| key == name) {
                Some(entry) => entry.1 = value,
                None => entries.push((name.to_string(), value)),
            }
        }
        Rc::new(DataFrame {
            entries,
            parent: Some(parent.clone()),
        })
    }
}

struct DepthNode<'data> {
    value: TemplateValue<'data>,
    parent: Option<Rc<DepthNode<'data>>>,
}

struct BlockParamFrame<'tree, 'data> {
    names: &'tree [String],
    values: Vec<TemplateValue<'data>>,
    parent: Option<Rc<BlockParamFrame<'tree, 'data>>>,
}

#[derive(Clone)]
struct Scope<'tree, 'data> {
    context: TemplateValue<'data>,
    depths: Rc<DepthNode<'data>>,
    data: Rc<DataFrame<'data>>,
    block_params: Option<Rc<BlockParamFrame<'tree, 'data>>>,
}

#[derive(Clone)]
struct ProgramHandle<'tree, 'data> {
    program: Option<&'tree Program>,
    depths: Rc<DepthNode<'data>>,
    data: Rc<DataFrame<'data>>,
    block_params: Option<Rc<BlockParamFrame<'tree, 'data>>>,
}

struct HelperOptions<'tree, 'data> {
    name: String,
    hash: Vec<(String, TemplateValue<'data>)>,
    function: ProgramHandle<'tree, 'data>,
    inverse: ProgramHandle<'tree, 'data>,
    data: Rc<DataFrame<'data>>,
    this: TemplateValue<'data>,
}

impl<'data> HelperOptions<'_, 'data> {
    fn hash_value(&self, name: &str) -> TemplateValue<'data> {
        self.hash
            .iter()
            .find(|(key, _)| key == name)
            .map_or(TemplateValue::Missing, |(_, value)| value.clone())
    }
}

#[derive(Clone, Copy)]
enum CallSite<'tree> {
    Mustache,
    SubExpression,
    Block(&'tree Block),
}

enum Argument<'call, 'data> {
    Value(&'call TemplateValue<'data>),
    Options,
    Missing,
}

fn argument<'call, 'data>(
    params: &'call [TemplateValue<'data>],
    index: usize,
) -> Argument<'call, 'data> {
    match index.cmp(&params.len()) {
        std::cmp::Ordering::Less => Argument::Value(&params[index]),
        std::cmp::Ordering::Equal => Argument::Options,
        std::cmp::Ordering::Greater => Argument::Missing,
    }
}

fn program_handle<'tree, 'data>(
    program: Option<&'tree Program>,
    scope: &Scope<'tree, 'data>,
) -> ProgramHandle<'tree, 'data> {
    ProgramHandle {
        program,
        depths: scope.depths.clone(),
        data: scope.data.clone(),
        block_params: scope.block_params.clone(),
    }
}

fn invoke<'tree, 'data>(
    handle: &ProgramHandle<'tree, 'data>,
    context: TemplateValue<'data>,
    data: Option<Rc<DataFrame<'data>>>,
    block_param_values: Vec<TemplateValue<'data>>,
) -> Result<String> {
    let Some(program) = handle.program else {
        return Ok(String::new());
    };
    let keeps_depths = loosely_equal(&context, &handle.depths.value)
        || (matches!(context, TemplateValue::NullContext)
            && matches!(handle.depths.value.value(), Some(Value::Null)));
    let depths = if keeps_depths {
        handle.depths.clone()
    } else {
        Rc::new(DepthNode {
            value: context.clone(),
            parent: Some(handle.depths.clone()),
        })
    };
    let names: &'tree [String] = program.block_params.as_deref().unwrap_or(&[]);
    let scope = Scope {
        context,
        depths,
        data: data.unwrap_or_else(|| handle.data.clone()),
        block_params: Some(Rc::new(BlockParamFrame {
            names,
            values: block_param_values,
            parent: handle.block_params.clone(),
        })),
    };
    render_program(program, &scope)
}

fn render_program<'tree>(program: &'tree Program, scope: &Scope<'tree, '_>) -> Result<String> {
    let mut output = String::new();
    for statement in &program.body {
        match statement {
            Statement::Content(content) => output.push_str(&content.value),
            Statement::Comment(_) => {}
            Statement::Mustache(mustache) => {
                let value = evaluate_call(&mustache.call, scope, CallSite::Mustache)?;
                match mustache.escaping {
                    OutputEscaping::Html => push_escaped_expression(&mut output, &value),
                    OutputEscaping::Raw => push_appended_string(&mut output, &value),
                }
            }
            Statement::Block(block) => {
                let value = evaluate_call(&block.call, scope, CallSite::Block(block))?;
                push_appended_string(&mut output, &value);
            }
        }
    }
    Ok(output)
}

fn is_scoped(path: &PathExpression) -> bool {
    if path.source_text.starts_with('.') {
        return true;
    }
    path.source_text.match_indices("this").any(|(position, _)| {
        !path.source_text[position + 4..]
            .chars()
            .next()
            .is_some_and(|next| next.is_ascii_alphanumeric() || next == '_')
    })
}

fn is_simple_identifier(path: &PathExpression) -> bool {
    path.parts.len() == 1 && !is_scoped(path) && path.depth == 0
}

fn call_path(call: &Call) -> Cow<'_, PathExpression> {
    match &call.path {
        Expression::Path(path) => Cow::Borrowed(path),
        literal => {
            let source_text = literal_source_text(literal);
            Cow::Owned(PathExpression {
                data: false,
                depth: 0,
                parts: vec![source_text.clone()],
                source_text,
            })
        }
    }
}

fn context_at_depth<'data>(scope: &Scope<'_, 'data>, depth: usize) -> TemplateValue<'data> {
    if depth == 0 {
        return scope.context.clone();
    }
    let mut node = Some(&scope.depths);
    for _ in 0..depth {
        node = node.and_then(|current| current.parent.as_ref());
    }
    node.map_or(TemplateValue::Missing, |current| current.value.clone())
}

fn data_at_depth<'data>(scope: &Scope<'_, 'data>, depth: usize) -> Option<Rc<DataFrame<'data>>> {
    let mut frame = Some(scope.data.clone());
    for _ in 0..depth {
        frame = frame.and_then(|current| current.parent.clone());
    }
    frame
}

fn find_block_param<'data>(scope: &Scope<'_, 'data>, name: &str) -> Option<TemplateValue<'data>> {
    let mut frame = scope.block_params.as_ref();
    while let Some(current) = frame {
        if let Some(position) = current.names.iter().position(|declared| declared == name) {
            return Some(
                current
                    .values
                    .get(position)
                    .cloned()
                    .unwrap_or(TemplateValue::Missing),
            );
        }
        frame = current.parent.as_ref();
    }
    None
}

#[derive(Clone, Copy)]
enum LookupStop {
    AtFalsy,
    AtNullish,
}

fn resolve_parts<'data>(
    start: TemplateValue<'data>,
    parts: &[String],
    stop: LookupStop,
) -> TemplateValue<'data> {
    let mut current = start;
    for part in parts {
        let stops = match stop {
            LookupStop::AtFalsy => !current.is_truthy(),
            LookupStop::AtNullish => current.is_nullish(),
        };
        if stops {
            return current;
        }
        current = lookup_property(&current, part);
    }
    current
}

fn lookup_path<'data>(path: &PathExpression, scope: &Scope<'_, 'data>) -> TemplateValue<'data> {
    if path.depth == 0
        && !is_scoped(path)
        && let Some(first) = path.parts.first()
        && let Some(value) = find_block_param(scope, first)
    {
        return resolve_parts(value, &path.parts[1..], LookupStop::AtNullish);
    }
    if path.parts.is_empty() {
        return context_at_depth(scope, path.depth);
    }
    if path.data {
        return match data_at_depth(scope, path.depth) {
            Some(frame) => resolve_parts(
                frame.get(&path.parts[0]),
                &path.parts[1..],
                LookupStop::AtFalsy,
            ),
            None => TemplateValue::Missing,
        };
    }
    resolve_parts(
        context_at_depth(scope, path.depth),
        &path.parts,
        LookupStop::AtNullish,
    )
}

fn evaluate_expression<'tree, 'data>(
    expression: &'tree Expression,
    scope: &Scope<'tree, 'data>,
) -> Result<TemplateValue<'data>> {
    Ok(match expression {
        Expression::Path(path) => lookup_path(path, scope),
        Expression::Invocation(call) => evaluate_call(call, scope, CallSite::SubExpression)?,
        Expression::StringLiteral(text) => TemplateValue::text(text.clone()),
        Expression::NumberLiteral(number) => TemplateValue::owned(number_literal_value(*number)),
        Expression::BooleanLiteral(flag) => TemplateValue::owned(Value::Bool(*flag)),
        Expression::MissingValueLiteral => TemplateValue::Missing,
        Expression::NullLiteral => TemplateValue::owned(Value::Null),
    })
}

fn helper_this<'data>(scope: &Scope<'_, 'data>) -> TemplateValue<'data> {
    if scope.context.is_nullish() {
        TemplateValue::NullContext
    } else {
        scope.context.clone()
    }
}

fn is_registered_helper(name: &str) -> bool {
    KNOWN_HELPERS.contains(&name) || MERGEABLE_HELPERS.contains(&name)
}

fn evaluate_call<'tree, 'data>(
    call: &'tree Call,
    scope: &Scope<'tree, 'data>,
    site: CallSite<'tree>,
) -> Result<TemplateValue<'data>> {
    let path = call_path(call);
    let is_simple = is_simple_identifier(&path);
    let first_part = path.parts.first().map(String::as_str).unwrap_or_default();
    let is_block_param = is_simple && find_block_param(scope, first_part).is_some();
    let has_arguments =
        matches!(site, CallSite::SubExpression) || !call.params.is_empty() || call.hash.is_some();
    let mut is_helper = !is_block_param && has_arguments;
    let is_eligible = !is_block_param && (is_helper || is_simple);
    if is_eligible && !is_helper && KNOWN_HELPERS.contains(&first_part) {
        is_helper = true;
    }
    let (function, inverse) = match site {
        CallSite::Block(block) => (
            program_handle(block.program.as_ref(), scope),
            program_handle(block.inverse.as_ref(), scope),
        ),
        _ => (program_handle(None, scope), program_handle(None, scope)),
    };
    let mut options = HelperOptions {
        name: String::new(),
        hash: Vec::new(),
        function,
        inverse,
        data: scope.data.clone(),
        this: helper_this(scope),
    };
    let is_block = matches!(site, CallSite::Block(_));
    if is_helper {
        let params = call
            .params
            .iter()
            .map(|param| evaluate_expression(param, scope))
            .collect::<Result<Vec<_>>>()?;
        if let Some(pairs) = &call.hash {
            for (key, expression) in pairs {
                options
                    .hash
                    .push((key.clone(), evaluate_expression(expression, scope)?));
            }
        }
        if KNOWN_HELPERS.contains(&first_part) || (is_simple && is_registered_helper(first_part)) {
            options.name = if KNOWN_HELPERS.contains(&first_part) {
                first_part.to_string()
            } else {
                path.source_text.clone()
            };
            return call_helper(first_part, &params, &options);
        }
        let value = lookup_path(&path, scope);
        if value.is_truthy() {
            return Err(Error::type_mismatch(format!(
                "{} is not a function",
                path.source_text
            )));
        }
        if params.is_empty() {
            return Ok(TemplateValue::Missing);
        }
        return Err(Error::general(format!(
            "Missing helper: \"{}\"",
            path.source_text
        )));
    }
    if is_eligible {
        if is_registered_helper(first_part) {
            options.name = first_part.to_string();
            return call_helper(first_part, &[], &options);
        }
        let value = lookup_path(&path, scope);
        let current = if value.is_nullish() {
            TemplateValue::Missing
        } else {
            value
        };
        return if is_block {
            block_helper_missing(current, &options)
        } else {
            Ok(current)
        };
    }
    let value = lookup_path(&path, scope);
    if is_block {
        block_helper_missing(value, &options)
    } else {
        Ok(value)
    }
}

fn call_helper<'data>(
    name: &str,
    params: &[TemplateValue<'data>],
    options: &HelperOptions<'_, 'data>,
) -> Result<TemplateValue<'data>> {
    match name {
        "if" => conditional_helper(params, options, Conditional::If),
        "unless" => conditional_helper(params, options, Conditional::Unless),
        "each" => each_helper(params, options),
        "with" => with_helper(params, options),
        "lookup" => Ok(lookup_helper(params)),
        "log" => Ok(TemplateValue::Missing),
        "blockHelperMissing" => block_helper_missing(
            params
                .first()
                .cloned()
                .unwrap_or(TemplateValue::NullContext),
            options,
        ),
        "breaklines" => Ok(breaklines_helper(params)),
        "toUpperCase" => to_upper_case_helper(params),
        "formatDate" => Ok(format_date_helper(params)),
        "displaySettings" => Ok(display_settings_helper(params, options)),
        "ifEquals" => if_equals_helper(params, options),
        "statusIcon" => Ok(status_icon_helper(params)),
        _ => {
            if params.is_empty() {
                Ok(TemplateValue::Missing)
            } else {
                Err(Error::general(format!(
                    "Missing helper: \"{}\"",
                    options.name
                )))
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Conditional {
    If,
    Unless,
}

impl Conditional {
    fn helper_name(self) -> &'static str {
        match self {
            Conditional::If => "if",
            Conditional::Unless => "unless",
        }
    }
}

fn conditional_helper<'data>(
    params: &[TemplateValue<'data>],
    options: &HelperOptions<'_, 'data>,
    conditional_kind: Conditional,
) -> Result<TemplateValue<'data>> {
    if params.len() != 1 {
        let helper_name = conditional_kind.helper_name();
        return Err(Error::general(format!(
            "#{helper_name} requires exactly one argument"
        )));
    }
    let conditional = &params[0];
    let include_zero = options.hash_value("includeZero").is_truthy();
    let is_falsy = (!include_zero && !conditional.is_truthy()) || is_empty(conditional);
    let (truthy_program, falsy_program) = match conditional_kind {
        Conditional::If => (&options.function, &options.inverse),
        Conditional::Unless => (&options.inverse, &options.function),
    };
    let chosen = if is_falsy {
        falsy_program
    } else {
        truthy_program
    };
    Ok(TemplateValue::text(invoke(
        chosen,
        options.this.clone(),
        None,
        Vec::new(),
    )?))
}

fn with_helper<'data>(
    params: &[TemplateValue<'data>],
    options: &HelperOptions<'_, 'data>,
) -> Result<TemplateValue<'data>> {
    if params.len() != 1 {
        return Err(Error::general("#with requires exactly one argument"));
    }
    let context = params[0].clone();
    let output = if is_empty(&context) {
        invoke(&options.inverse, options.this.clone(), None, Vec::new())?
    } else {
        invoke(
            &options.function,
            context.clone(),
            Some(options.data.clone()),
            vec![context],
        )?
    };
    Ok(TemplateValue::text(output))
}

fn each_helper<'data>(
    params: &[TemplateValue<'data>],
    options: &HelperOptions<'_, 'data>,
) -> Result<TemplateValue<'data>> {
    let Some(context) = params.first() else {
        return Err(Error::general("Must pass iterator to #each"));
    };
    if params.len() > 1 {
        return Err(Error::type_mismatch("fn is not a function"));
    }
    let mut output = String::new();
    let mut iterations = 0;
    match context.value() {
        Some(Value::Array(items)) => {
            let length = items.len();
            for index in 0..length {
                let item =
                    context.child(|value| value.as_array().and_then(|items| items.get(index)));
                let position = TemplateValue::owned(Value::from(index));
                let frame = DataFrame::child_frame(
                    &options.data,
                    [
                        ("key", position.clone()),
                        ("index", position.clone()),
                        ("first", TemplateValue::owned(Value::Bool(index == 0))),
                        (
                            "last",
                            TemplateValue::owned(Value::Bool(index + 1 == length)),
                        ),
                    ],
                );
                output.push_str(&invoke(
                    &options.function,
                    item.clone(),
                    Some(frame),
                    vec![item, position],
                )?);
                iterations += 1;
            }
        }
        Some(Value::Object(map)) => {
            let keys = keys_in_property_order(map);
            let length = keys.len();
            for (index, key) in keys.into_iter().enumerate() {
                let item = context.child(|value| value.get(key.as_str()));
                let key_value = TemplateValue::text(key.clone());
                let frame = DataFrame::child_frame(
                    &options.data,
                    [
                        ("key", key_value.clone()),
                        ("index", TemplateValue::owned(Value::from(index))),
                        ("first", TemplateValue::owned(Value::Bool(index == 0))),
                        (
                            "last",
                            TemplateValue::owned(Value::Bool(index + 1 == length)),
                        ),
                    ],
                );
                output.push_str(&invoke(
                    &options.function,
                    item.clone(),
                    Some(frame),
                    vec![item, key_value],
                )?);
                iterations += 1;
            }
        }
        _ => {}
    }
    if iterations == 0 {
        output = invoke(&options.inverse, options.this.clone(), None, Vec::new())?;
    }
    Ok(TemplateValue::text(output))
}

fn lookup_helper<'data>(params: &[TemplateValue<'data>]) -> TemplateValue<'data> {
    let Some(object) = params.first() else {
        return TemplateValue::Missing;
    };
    if !object.is_truthy() {
        return object.clone();
    }
    let field = params
        .get(1)
        .map_or(Cow::Borrowed("[object Object]"), display_text);
    lookup_property(object, &field)
}

fn block_helper_missing<'data>(
    context: TemplateValue<'data>,
    options: &HelperOptions<'_, 'data>,
) -> Result<TemplateValue<'data>> {
    let output = match context.value() {
        Some(Value::Bool(true)) => {
            invoke(&options.function, options.this.clone(), None, Vec::new())?
        }
        Some(Value::Bool(false) | Value::Null) => {
            invoke(&options.inverse, options.this.clone(), None, Vec::new())?
        }
        None if matches!(context, TemplateValue::Missing) => {
            invoke(&options.inverse, options.this.clone(), None, Vec::new())?
        }
        Some(Value::Array(items)) if items.is_empty() => {
            invoke(&options.inverse, options.this.clone(), None, Vec::new())?
        }
        Some(Value::Array(_)) => return each_helper(&[context], options),
        _ => invoke(&options.function, context, None, Vec::new())?,
    };
    Ok(TemplateValue::text(output))
}

fn breaklines_helper<'data>(params: &[TemplateValue<'data>]) -> TemplateValue<'data> {
    let mut escaped = String::new();
    match argument(params, 0) {
        Argument::Value(value) => push_escaped_expression(&mut escaped, value),
        Argument::Options => escaped.push_str("[object Object]"),
        Argument::Missing => {}
    }
    let mut with_breaks = String::with_capacity(escaped.len());
    let mut characters = escaped.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                with_breaks.push_str("<br>");
            }
            '\n' => with_breaks.push_str("<br>"),
            other => with_breaks.push(other),
        }
    }
    TemplateValue::Safe(Rc::from(with_breaks.as_str()))
}

fn to_upper_case_helper<'data>(params: &[TemplateValue<'data>]) -> Result<TemplateValue<'data>> {
    match argument(params, 0) {
        Argument::Value(TemplateValue::Missing) | Argument::Missing => {
            Err(Error::missing_property("toUpperCase"))
        }
        Argument::Value(value) if value.is_nullish() => Err(Error::type_mismatch(
            "Cannot read properties of null (reading 'toUpperCase')",
        )),
        Argument::Value(value) => match value.value() {
            Some(Value::String(text)) => Ok(TemplateValue::text(text.to_uppercase())),
            _ => Err(Error::type_mismatch("str.toUpperCase is not a function")),
        },
        Argument::Options => Err(Error::type_mismatch("str.toUpperCase is not a function")),
    }
}

fn format_date_helper<'data>(params: &[TemplateValue<'data>]) -> TemplateValue<'data> {
    match argument(params, 0) {
        Argument::Value(TemplateValue::Missing) | Argument::Missing => TemplateValue::Missing,
        Argument::Value(value) => match value.value() {
            Some(Value::String(text)) => TemplateValue::text(format_date(text)),
            _ => TemplateValue::text(format_instant(Utc::now())),
        },
        Argument::Options => TemplateValue::text(format_instant(Utc::now())),
    }
}

fn display_settings_helper<'data>(
    params: &[TemplateValue<'data>],
    options: &HelperOptions<'_, 'data>,
) -> TemplateValue<'data> {
    let serialized = match argument(params, 0) {
        Argument::Value(TemplateValue::Missing) | Argument::Missing => "undefined".to_string(),
        Argument::Value(TemplateValue::Safe(text)) => format!(
            "{{\"string\":{}}}",
            serde_json::to_string(text.as_ref()).unwrap_or_default()
        ),
        Argument::Value(TemplateValue::NullContext) => "{}".to_string(),
        Argument::Value(value) => value.value().map(json_stringify).unwrap_or_default(),
        Argument::Options => format!(
            "{{\"name\":{},\"hash\":{{}},\"data\":{{\"root\":{}}}}}",
            serde_json::to_string(&options.name).unwrap_or_default(),
            options
                .data
                .get("root")
                .value()
                .map(json_stringify)
                .unwrap_or_default()
        ),
    };
    TemplateValue::text(format!("\x60\x60\x60{serialized}\x60\x60\x60"))
}

fn if_equals_helper<'data>(
    params: &[TemplateValue<'data>],
    options: &HelperOptions<'_, 'data>,
) -> Result<TemplateValue<'data>> {
    let left = argument(params, 0);
    let right = argument(params, 1);
    let equal = match (&left, &right) {
        (Argument::Value(first), Argument::Value(second)) => strictly_equal(first, second),
        (Argument::Value(TemplateValue::Missing), Argument::Missing)
        | (Argument::Missing, Argument::Value(TemplateValue::Missing)) => true,
        _ => false,
    };
    let member = if equal { "fn" } else { "inverse" };
    match argument(params, 2) {
        Argument::Options => {
            let chosen = if equal {
                &options.function
            } else {
                &options.inverse
            };
            Ok(TemplateValue::text(invoke(
                chosen,
                options.this.clone(),
                None,
                Vec::new(),
            )?))
        }
        Argument::Missing | Argument::Value(TemplateValue::Missing) => {
            Err(Error::missing_property(member))
        }
        Argument::Value(value) if value.is_nullish() => Err(Error::type_mismatch(format!(
            "Cannot read properties of null (reading '{member}')"
        ))),
        Argument::Value(_) => Err(Error::type_mismatch(format!(
            "options.{member} is not a function"
        ))),
    }
}

fn status_icon_helper<'data>(params: &[TemplateValue<'data>]) -> TemplateValue<'data> {
    let status = match argument(params, 0) {
        Argument::Value(value) => value,
        Argument::Options => return TemplateValue::text("Unknown Status given: [object Object]"),
        Argument::Missing => return TemplateValue::text("Unknown Status given: undefined"),
    };
    let icon = match status.value() {
        Some(Value::String(text)) if text == "pass" => ":heavy_check_mark:",
        Some(Value::String(text)) if text == "fail" => ":x:",
        Some(Value::String(text)) if text == "error" => ":heavy_exclamation_mark:",
        Some(Value::String(text)) if text == "info" => ":information_source:",
        _ => {
            return TemplateValue::text(format!(
                "Unknown Status given: {}",
                to_display_string(status)
            ));
        }
    };
    TemplateValue::text(icon)
}

fn format_date(text: &str) -> String {
    match parse_date_text(text) {
        Some(instant) => format_instant(instant),
        None => "Invalid Date".to_string(),
    }
}

fn format_instant(instant: DateTime<Utc>) -> String {
    let year = instant.year();
    let displayed_year = if year <= 0 { 1 - year } else { year };
    let hour = instant.hour();
    let (twelve_hour, meridiem) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!(
        "{} {}, {}, {}:{:02} {}",
        MONTH_ABBREVIATIONS[instant.month0() as usize],
        instant.day(),
        displayed_year,
        twelve_hour,
        instant.minute(),
        meridiem
    )
}

fn parse_date_text(text: &str) -> Option<DateTime<Utc>> {
    let instant = parse_iso_date(text).or_else(|| {
        DateTime::parse_from_rfc2822(text)
            .ok()
            .map(|parsed| parsed.with_timezone(&Utc))
    })?;
    (instant.timestamp_millis().abs() <= MAXIMUM_DATE_MILLISECONDS).then_some(instant)
}

struct DigitReader<'text> {
    bytes: &'text [u8],
    position: usize,
}

impl DigitReader<'_> {
    fn digits(&mut self, count: usize) -> Option<i64> {
        let slice = self.bytes.get(self.position..self.position + count)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.position += count;
        slice.iter().try_fold(0_i64, |total, digit| {
            total.checked_mul(10)?.checked_add(i64::from(digit - b'0'))
        })
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn eat(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }
}

fn parse_iso_date(text: &str) -> Option<DateTime<Utc>> {
    let mut reader = DigitReader {
        bytes: text.as_bytes(),
        position: 0,
    };
    let year = match reader.peek()? {
        sign @ (b'+' | b'-') => {
            reader.position += 1;
            let magnitude = reader.digits(6)?;
            if sign == b'-' && magnitude == 0 {
                return None;
            }
            if sign == b'-' { -magnitude } else { magnitude }
        }
        _ => reader.digits(4)?,
    };
    let mut month = 1;
    let mut day = 1;
    if reader.eat(b'-') {
        month = reader.digits(2)?;
        if reader.eat(b'-') {
            day = reader.digits(2)?;
        }
    }
    let mut time = None;
    let mut offset_minutes = None;
    if reader.eat(b'T') || reader.eat(b't') {
        let hour = reader.digits(2)?;
        if !reader.eat(b':') {
            return None;
        }
        let minute = reader.digits(2)?;
        let mut second = 0;
        let mut millisecond = 0;
        if reader.eat(b':') {
            second = reader.digits(2)?;
            if reader.eat(b'.') {
                let start = reader.position;
                while reader.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                    reader.position += 1;
                }
                let fraction = &text[start..reader.position];
                if fraction.is_empty() {
                    return None;
                }
                let padded = format!("{fraction:0<3}");
                millisecond = padded[..3].parse::<i64>().ok()?;
            }
        }
        if reader.eat(b'Z') || reader.eat(b'z') {
            offset_minutes = Some(0);
        } else if let Some(sign @ (b'+' | b'-')) = reader.peek() {
            reader.position += 1;
            let offset_hours = reader.digits(2)?;
            reader.eat(b':');
            let offset_remainder = reader.digits(2)?;
            let total = offset_hours * 60 + offset_remainder;
            offset_minutes = Some(if sign == b'-' { -total } else { total });
        }
        if hour > 24
            || minute > 59
            || second > 59
            || (hour == 24 && (minute > 0 || second > 0 || millisecond > 0))
        {
            return None;
        }
        time = Some(
            Duration::hours(hour)
                + Duration::minutes(minute)
                + Duration::seconds(second)
                + Duration::milliseconds(millisecond),
        );
    }
    if reader.position != text.len() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let first_of_month =
        NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, u32::try_from(month).ok()?, 1)?;
    let midnight: NaiveDateTime = first_of_month
        .checked_add_signed(Duration::days(day - 1))?
        .and_hms_opt(0, 0, 0)?;
    match (time, offset_minutes) {
        (None, _) => Some(Utc.from_utc_datetime(&midnight)),
        (Some(time_of_day), Some(offset)) => {
            Some(Utc.from_utc_datetime(&(midnight + time_of_day - Duration::minutes(offset))))
        }
        (Some(time_of_day), None) => {
            let local = midnight + time_of_day;
            let resolved = Local.from_local_datetime(&local).earliest().or_else(|| {
                Local
                    .from_local_datetime(&(local + Duration::hours(1)))
                    .earliest()
            })?;
            Some(resolved.with_timezone(&Utc))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEMPLATE_CORPUS: &str = include_str!("../tests/fixtures/templates.json");

    fn render_on_a_one_megabyte_stack(template: String) -> Result<String> {
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || render(&template, &json!({"flag": true})))
            .unwrap()
            .join()
            .unwrap()
    }

    fn nested_if_blocks(depth: usize) -> String {
        format!(
            "{}x{}",
            "{{#if flag}}".repeat(depth),
            "{{/if}}".repeat(depth)
        )
    }

    fn nested_subexpressions(depth: usize) -> String {
        format!(
            "{{{{lookup {}flag{}}}}}",
            "(lookup ".repeat(depth),
            ")".repeat(depth)
        )
    }

    fn chained_else_if_blocks(length: usize) -> String {
        format!(
            "{{{{#if missing}}}}a{}{{{{else}}}}x{{{{/if}}}}",
            "{{else if missing}}b".repeat(length)
        )
    }

    #[test]
    fn templates_nested_up_to_the_limit_render() {
        assert_eq!(
            render_on_a_one_megabyte_stack(nested_if_blocks(MAXIMUM_NESTING_DEPTH)).unwrap(),
            "x"
        );
        assert!(
            render_on_a_one_megabyte_stack(nested_subexpressions(MAXIMUM_NESTING_DEPTH - 1))
                .is_ok()
        );
        assert_eq!(
            render_on_a_one_megabyte_stack(chained_else_if_blocks(MAXIMUM_NESTING_DEPTH - 1))
                .unwrap(),
            "x"
        );
    }

    #[test]
    fn deeply_nested_templates_fail_with_a_range_error_instead_of_overflowing_the_stack() {
        for template in [
            nested_if_blocks(50_000),
            nested_subexpressions(50_000),
            chained_else_if_blocks(50_000),
        ] {
            let error = render_on_a_one_megabyte_stack(template).unwrap_err();
            assert_eq!(
                error.to_string(),
                "RangeError: Maximum call stack size exceeded"
            );
        }
    }

    #[test]
    fn every_corpus_template_renders_as_recorded() {
        let corpus: Vec<Value> = serde_json::from_str(TEMPLATE_CORPUS).unwrap();
        let mut mismatches = Vec::new();
        for entry in &corpus {
            let name = entry["name"].as_str().unwrap_or_default();
            let template = entry["template"].as_str().unwrap_or_default();
            let rendered = render(template, &entry["data"]);
            match (
                entry.get("expected").and_then(Value::as_str),
                entry.get("error").and_then(Value::as_str),
                rendered,
            ) {
                (Some(expected), _, Ok(actual)) if actual == expected => {}
                (Some(expected), _, outcome) => {
                    mismatches.push(format!("{name}: expected {expected:?}, got {outcome:?}"));
                }
                (None, Some(expected_error), Err(actual_error)) => {
                    let is_parse_error = expected_error.starts_with("Error: Parse error");
                    if !is_parse_error && actual_error.to_string() != expected_error {
                        mismatches.push(format!(
                            "{name}: expected error {expected_error:?}, got {actual_error}"
                        ));
                    }
                }
                (None, expected_error, outcome) => mismatches.push(format!(
                    "{name}: expected error {expected_error:?}, got {outcome:?}"
                )),
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} of {} corpus entries differ:\n{}",
            mismatches.len(),
            corpus.len(),
            mismatches.join("\n")
        );
    }

    #[test]
    fn format_date_without_argument_renders_the_current_time() {
        let rendered = render("{{formatDate}}", &json!({})).unwrap();
        let expected_prefix = MONTH_ABBREVIATIONS[Utc::now().month0() as usize];
        assert!(rendered.starts_with(expected_prefix), "{rendered}");
        assert!(
            rendered.ends_with(" AM") || rendered.ends_with(" PM"),
            "{rendered}"
        );
    }

    #[test]
    fn format_date_of_a_non_string_renders_the_current_time() {
        let rendered = render("{{formatDate number}}", &json!({"number": 5})).unwrap();
        assert!(
            rendered.ends_with(" AM") || rendered.ends_with(" PM"),
            "{rendered}"
        );
    }

    #[test]
    fn populate_template_merges_payload_with_results_and_replaces_annotations() {
        let payload = json!({"user": {"login": "alice"}, "title": "payload title", "labels": [{"name": "a"}, {"name": "b"}]});
        let results = json!({"status": "pass", "labels": [{"name": "x"}]});
        let event = json!({"action": "opened", "repository": {"full_name": "acme/app"}, "sender": {"login": "bob"}});
        let rendered = populate_template(
            &json!("@author {{title}} {{status}} {{#each labels}}{{name}}{{/each}} @sender"),
            &results,
            Some(&payload),
            &crate::event_aware::EventAttributes::from_payload(&event),
        )
        .unwrap();
        assert_eq!(rendered, "alice payload title pass xb bob");
    }

    #[test]
    fn populate_template_without_template_is_a_type_error() {
        let error = populate_template(
            &Value::Null,
            &json!({}),
            Some(&json!({"user": {"login": "a"}})),
            &crate::event_aware::EventAttributes::default(),
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "TypeError: Cannot read properties of undefined (reading 'replace')"
        );
    }

    #[test]
    fn compiling_a_non_string_template_fails() {
        let error = compile_and_render(&json!(5), &json!({})).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Error: You must pass a string or Handlebars AST to Handlebars.compile. You passed 5"
        );
    }

    #[test]
    fn merge_commit_templates_do_not_escape_html() {
        let pull_request = json!({"title": "some pr", "body": "some <> ' message", "number": 10});
        assert_eq!(
            render("{{{ title }}} (#{{{ number }}})", &pull_request).unwrap(),
            "some pr (#10)"
        );
        assert_eq!(
            render("{{{ body }}}", &pull_request).unwrap(),
            "some <> ' message"
        );
        assert_eq!(render("", &pull_request).unwrap(), "");
    }
}
