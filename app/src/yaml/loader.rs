use super::mark::{self, Mark};
use super::types::{IMPLICIT_TYPES, MERGE_TAG, TypeContext, YamlType};
use super::value::{Heap, JsValue};
use super::{Dialect, YamlError};
use std::collections::{HashMap, HashSet};

const NOT_A_CHARACTER: u32 = u32::MAX;
const NUL: u32 = 0x00;
const TAB: u32 = 0x09;
const LINE_FEED: u32 = 0x0A;
const CARRIAGE_RETURN: u32 = 0x0D;
const SPACE: u32 = 0x20;
const EXCLAMATION: u32 = 0x21;
const DOUBLE_QUOTE: u32 = 0x22;
const HASH: u32 = 0x23;
const PERCENT: u32 = 0x25;
const AMPERSAND: u32 = 0x26;
const SINGLE_QUOTE: u32 = 0x27;
const ASTERISK: u32 = 0x2A;
const PLUS: u32 = 0x2B;
const COMMA: u32 = 0x2C;
const HYPHEN: u32 = 0x2D;
const PERIOD: u32 = 0x2E;
const COLON: u32 = 0x3A;
const LESS_THAN: u32 = 0x3C;
const GREATER_THAN: u32 = 0x3E;
const QUESTION_MARK: u32 = 0x3F;
const AT_SIGN: u32 = 0x40;
const LEFT_BRACKET: u32 = 0x5B;
const BACKSLASH: u32 = 0x5C;
const RIGHT_BRACKET: u32 = 0x5D;
const BACKTICK: u32 = 0x60;
const LEFT_BRACE: u32 = 0x7B;
const VERTICAL_BAR: u32 = 0x7C;
const RIGHT_BRACE: u32 = 0x7D;
const BYTE_ORDER_MARK: u16 = 0xFEFF;
// ponytail: V8 overflows its stack at roughly 1700-1900 nested nodes; a fixed 1000 approximates that RangeError
const MAXIMUM_NESTING_DEPTH: usize = 1000;
const CORE_TAG_PREFIX: &str = "tag:yaml.org,2002:";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Scalar,
    Sequence,
    Mapping,
}

fn kind_name(kind: Option<Kind>) -> &'static str {
    match kind {
        None => "null",
        Some(Kind::Scalar) => "scalar",
        Some(Kind::Sequence) => "sequence",
        Some(Kind::Mapping) => "mapping",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeContext {
    FlowIn,
    FlowOut,
    BlockIn,
    BlockOut,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Chomping {
    Clip,
    Strip,
    Keep,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IndentStatus {
    Deeper,
    Same,
    Shallower,
}

#[derive(Debug)]
pub(super) enum LoadError {
    Yaml { reason: String, mark: Mark },
    Thrown { name: &'static str, message: String },
}

#[derive(Clone, Copy, Default)]
struct KeyStart {
    line: Option<usize>,
    line_start: Option<usize>,
    position: Option<usize>,
}

impl KeyStart {
    fn truthy(value: Option<usize>) -> Option<usize> {
        value.filter(|number| *number != 0)
    }
}

type LoadResult<T> = Result<T, LoadError>;

fn is_eol(character: u32) -> bool {
    character == LINE_FEED || character == CARRIAGE_RETURN
}

fn is_white_space(character: u32) -> bool {
    character == TAB || character == SPACE
}

fn is_ws_or_eol(character: u32) -> bool {
    is_white_space(character) || is_eol(character)
}

fn is_flow_indicator(character: u32) -> bool {
    matches!(
        character,
        COMMA | LEFT_BRACKET | RIGHT_BRACKET | LEFT_BRACE | RIGHT_BRACE
    )
}

fn from_hex_code(character: u32) -> Option<i64> {
    char::from_u32(character)
        .and_then(|digit| digit.to_digit(16))
        .map(i64::from)
}

fn escaped_hex_length(character: u32) -> usize {
    match character {
        0x78 => 2,
        0x75 => 4,
        0x55 => 8,
        _ => 0,
    }
}

fn simple_escape_sequence(character: u32) -> Option<u16> {
    Some(match character {
        0x30 => 0x00,
        0x61 => 0x07,
        0x62 => 0x08,
        0x74 | TAB => 0x09,
        0x6E => 0x0A,
        0x76 => 0x0B,
        0x66 => 0x0C,
        0x72 => 0x0D,
        0x65 => 0x1B,
        SPACE => 0x20,
        DOUBLE_QUOTE => 0x22,
        0x2F => 0x2F,
        BACKSLASH => 0x5C,
        0x4E => 0x85,
        0x5F => 0xA0,
        0x4C => 0x2028,
        0x50 => 0x2029,
        _ => return None,
    })
}

fn to_int32(value: i64) -> i32 {
    let wrapped = value.rem_euclid(1 << 32);
    u32::try_from(wrapped).map_or(0, |unsigned| i32::from_ne_bytes(unsigned.to_ne_bytes()))
}

fn to_uint16(value: i64) -> u16 {
    u16::try_from(value.rem_euclid(1 << 16)).unwrap_or_default()
}

fn units_from_code_point(code_point: i64) -> Vec<u16> {
    if code_point <= 0xFFFF {
        return vec![to_uint16(code_point)];
    }
    let offset = code_point - 0x0001_0000;
    vec![
        to_uint16(i64::from(to_int32(offset) >> 10) + 0xD800),
        to_uint16(i64::from(to_int32(offset) & 0x03FF) + 0xDC00),
    ]
}

fn is_non_printable(segment: &[u16]) -> bool {
    segment.iter().enumerate().any(|(index, &unit)| {
        let is_high_surrogate = (0xD800..=0xDBFF).contains(&unit);
        let is_low_surrogate = (0xDC00..=0xDFFF).contains(&unit);
        matches!(unit, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F..=0x84 | 0x86..=0x9F | 0xFFFE | 0xFFFF)
            || (is_high_surrogate
                && !segment
                    .get(index + 1)
                    .is_some_and(|next| (0xDC00..=0xDFFF).contains(next)))
            || (is_low_surrogate
                && !index
                    .checked_sub(1)
                    .and_then(|previous| segment.get(previous))
                    .is_some_and(|previous| (0xD800..=0xDBFF).contains(previous)))
    })
}

fn is_tag_handle(handle: &[u16]) -> bool {
    let text = String::from_utf16_lossy(handle);
    if text == "!" || text == "!!" {
        return true;
    }
    text.len() > 2
        && text.starts_with('!')
        && text.ends_with('!')
        && text.get(1..text.len() - 1).is_some_and(|name| {
            name.bytes()
                .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
        })
}

fn is_tag_uri_character(unit: u16) -> bool {
    u8::try_from(unit)
        .is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"-#;/?:@&=+$,_.!~*'()[]".contains(&byte))
}

fn is_tag_uri(text: &[u16]) -> bool {
    let Some((&first, rest)) = text.split_first() else {
        return false;
    };
    let first_is_valid = first == 0x21 || !matches!(first, 0x2C | 0x5B | 0x5D | 0x7B | 0x7D);
    if !first_is_valid {
        return false;
    }
    let mut index = 0;
    while index < rest.len() {
        let unit = rest.get(index).copied().unwrap_or_default();
        if unit == 0x25 {
            let is_hex = |offset: usize| {
                rest.get(index + offset)
                    .and_then(|digit| u8::try_from(*digit).ok())
                    .is_some_and(|digit| digit.is_ascii_hexdigit())
            };
            if is_hex(1) && is_hex(2) {
                index += 3;
                continue;
            }
            return false;
        }
        if !is_tag_uri_character(unit) {
            return false;
        }
        index += 1;
    }
    true
}

fn decode_uri_component(text: &[u16]) -> Option<Vec<u16>> {
    let mut decoded: Vec<u16> = Vec::with_capacity(text.len());
    let mut index = 0;
    let read_byte = |at: usize| -> Option<u8> {
        if text.get(at) != Some(&0x25) {
            return None;
        }
        let high = text
            .get(at + 1)
            .and_then(|unit| char::from_u32(u32::from(*unit)))?
            .to_digit(16)?;
        let low = text
            .get(at + 2)
            .and_then(|unit| char::from_u32(u32::from(*unit)))?
            .to_digit(16)?;
        u8::try_from(high * 16 + low).ok()
    };
    while index < text.len() {
        let unit = text.get(index).copied().unwrap_or_default();
        if unit != 0x25 {
            decoded.push(unit);
            index += 1;
            continue;
        }
        let leading = read_byte(index)?;
        index += 3;
        let continuation_count = match leading {
            0x00..=0x7F => 0,
            0xC0..=0xDF => 1,
            0xE0..=0xEF => 2,
            0xF0..=0xF7 => 3,
            _ => return None,
        };
        let mut bytes = vec![leading];
        for _ in 0..continuation_count {
            let continuation = read_byte(index)?;
            if continuation & 0xC0 != 0x80 {
                return None;
            }
            bytes.push(continuation);
            index += 3;
        }
        let character = std::str::from_utf8(&bytes).ok()?;
        decoded.extend(character.encode_utf16());
    }
    Some(decoded)
}

fn to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn text_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

pub(super) fn load_documents(
    text: &str,
    dialect: Dialect,
) -> Result<(Vec<JsValue>, Heap), YamlError> {
    let mut loader = Loader::new(text, dialect);
    match loader.load_documents() {
        Ok(()) => Ok((loader.documents, loader.heap)),
        Err(LoadError::Yaml { reason, mark }) => Err(YamlError::yaml_exception(match dialect {
            Dialect::JsYaml3 => mark::format_js_yaml_3(&reason, &mark),
            Dialect::JsYaml4 => mark::format_js_yaml_4(&reason, &mark),
        })),
        Err(LoadError::Thrown { name, message }) => Err(YamlError { name, message }),
    }
}

struct Loader {
    dialect: Dialect,
    input: Vec<u16>,
    length: usize,
    position: usize,
    line: usize,
    line_start: usize,
    line_indent: i64,
    first_tab_in_line: Option<usize>,
    version: Option<String>,
    tag_map: HashMap<String, String>,
    anchor_map: HashMap<String, JsValue>,
    tag: Option<String>,
    anchor: Option<String>,
    kind: Option<Kind>,
    result: JsValue,
    documents: Vec<JsValue>,
    heap: Heap,
    nesting_depth: usize,
}

impl Loader {
    fn new(text: &str, dialect: Dialect) -> Self {
        let mut input: Vec<u16> = text.encode_utf16().collect();
        if let Some(&last) = input.last() {
            if u32::from(last) != LINE_FEED && u32::from(last) != CARRIAGE_RETURN {
                input.push(0x0A);
            }
            if input.first() == Some(&BYTE_ORDER_MARK) {
                input.remove(0);
            }
        }
        let length = input.len();
        Self {
            dialect,
            input,
            length,
            position: 0,
            line: 0,
            line_start: 0,
            line_indent: 0,
            first_tab_in_line: None,
            version: None,
            tag_map: HashMap::new(),
            anchor_map: HashMap::new(),
            tag: None,
            anchor: None,
            kind: None,
            result: JsValue::Null,
            documents: Vec::new(),
            heap: Heap::default(),
            nesting_depth: 0,
        }
    }

    fn is_js_yaml_4(&self) -> bool {
        self.dialect == Dialect::JsYaml4
    }

    fn character_at(&self, position: usize) -> u32 {
        self.input
            .get(position)
            .map_or(NOT_A_CHARACTER, |unit| u32::from(*unit))
    }

    fn current(&self) -> u32 {
        self.character_at(self.position)
    }

    fn advance(&mut self) -> u32 {
        self.position += 1;
        self.current()
    }

    fn slice(&self, start: usize, end: usize) -> Vec<u16> {
        self.input.get(start..end).unwrap_or_default().to_vec()
    }

    fn error(&self, reason: impl Into<String>) -> LoadError {
        let buffer = match self.dialect {
            Dialect::JsYaml3 => self.input.clone(),
            Dialect::JsYaml4 => self
                .input
                .get(..self.input.len().saturating_sub(1))
                .unwrap_or_default()
                .to_vec(),
        };
        LoadError::Yaml {
            reason: reason.into(),
            mark: Mark {
                buffer,
                position: self.position,
                line: self.line,
                column: to_i64(self.position) - to_i64(self.line_start),
            },
        }
    }

    fn fail<T>(&self, reason: impl Into<String>) -> LoadResult<T> {
        Err(self.error(reason))
    }

    fn register_anchor(&mut self, anchor: String, value: JsValue) {
        if self.dialect == Dialect::JsYaml3 && anchor == "__proto__" {
            return;
        }
        self.anchor_map.insert(anchor, value);
    }

    fn register_current_anchor(&mut self, value: JsValue) {
        if let Some(anchor) = self.anchor.clone() {
            self.register_anchor(anchor, value);
        }
    }

    fn load_documents(&mut self) -> LoadResult<()> {
        if let Some(null_position) = self.input.iter().position(|unit| *unit == 0) {
            self.position = null_position;
            return self.fail("null byte is not allowed in input");
        }
        self.input.push(0);
        while self.current() == SPACE {
            self.line_indent += 1;
            self.position += 1;
        }
        while self.position + 1 < self.length {
            self.read_document()?;
        }
        Ok(())
    }

    fn capture_segment(
        &self,
        start: usize,
        end: usize,
        check_json: bool,
        buffer: &mut Vec<u16>,
    ) -> LoadResult<()> {
        if start >= end {
            return Ok(());
        }
        let segment = self.input.get(start..end).unwrap_or_default();
        if check_json {
            if segment.iter().any(|unit| *unit != 0x09 && *unit < 0x20) {
                return self.fail("expected valid JSON character");
            }
        } else if is_non_printable(segment) {
            return self.fail("the stream contains non-printable characters");
        }
        buffer.extend_from_slice(segment);
        Ok(())
    }

    fn assign_property(&mut self, object_id: usize, key: String, value: JsValue) {
        if self.dialect == Dialect::JsYaml3 && key == "__proto__" {
            return;
        }
        self.heap.objects[object_id].set(key, value);
    }

    fn merge_mappings(
        &mut self,
        destination: usize,
        source: &JsValue,
        overridable_keys: &mut HashSet<String>,
    ) -> LoadResult<()> {
        if !source.is_js_object() {
            return self.fail("cannot merge mappings; the provided source object is unacceptable");
        }
        for (key, value) in self.heap.own_enumerable_entries(source) {
            if !self.heap.objects[destination].has_own(&key) {
                if key != "__proto__" {
                    self.heap.objects[destination].set(key.clone(), value);
                }
                overridable_keys.insert(key);
            }
        }
        Ok(())
    }

    fn mapping_key_string(&self, key_node: &JsValue) -> LoadResult<String> {
        match key_node {
            JsValue::Array(id) => {
                let mut converted = Vec::new();
                for item in &self.heap.arrays[*id] {
                    match item {
                        JsValue::Array(_) => {
                            return self.fail("nested arrays are not supported inside keys");
                        }
                        JsValue::Object(_) => {
                            converted.push(JsValue::String("[object Object]".to_string()));
                        }
                        other => converted.push(other.clone()),
                    }
                }
                Ok(converted
                    .iter()
                    .map(|item| match item {
                        JsValue::Null => String::new(),
                        other => self.heap.to_js_string(other, self.dialect),
                    })
                    .collect::<Vec<_>>()
                    .join(","))
            }
            JsValue::Object(_) => Ok("[object Object]".to_string()),
            other => Ok(self.heap.to_js_string(other, self.dialect)),
        }
    }

    fn store_mapping_pair(
        &mut self,
        target: Option<usize>,
        overridable_keys: &mut HashSet<String>,
        key_tag: Option<&str>,
        key_node: &JsValue,
        value_node: JsValue,
        key_start: KeyStart,
    ) -> LoadResult<usize> {
        let key = self.mapping_key_string(key_node)?;
        let object_id = target.unwrap_or_else(|| self.heap.new_object());
        if key_tag == Some(MERGE_TAG) {
            if let JsValue::Array(id) = &value_node {
                let sources = self.heap.arrays[*id].clone();
                for source in &sources {
                    self.merge_mappings(object_id, source, overridable_keys)?;
                }
            } else {
                self.merge_mappings(object_id, &value_node, overridable_keys)?;
            }
        } else {
            if !overridable_keys.contains(&key) && self.heap.objects[object_id].has_own(&key) {
                self.line = KeyStart::truthy(key_start.line).unwrap_or(self.line);
                self.line_start = KeyStart::truthy(key_start.line_start).unwrap_or(self.line_start);
                self.position = KeyStart::truthy(key_start.position).unwrap_or(self.position);
                return self.fail("duplicated mapping key");
            }
            self.assign_property(object_id, key.clone(), value_node);
            overridable_keys.remove(&key);
        }
        Ok(object_id)
    }

    fn read_line_break(&mut self) -> LoadResult<()> {
        let character = self.current();
        if character == LINE_FEED {
            self.position += 1;
        } else if character == CARRIAGE_RETURN {
            self.position += 1;
            if self.current() == LINE_FEED {
                self.position += 1;
            }
        } else {
            return self.fail("a line break is expected");
        }
        self.line += 1;
        self.line_start = self.position;
        if self.is_js_yaml_4() {
            self.first_tab_in_line = None;
        }
        Ok(())
    }

    fn skip_separation_space(&mut self, allow_comments: bool) -> LoadResult<usize> {
        let mut line_breaks = 0;
        let mut character = self.current();
        while character != NUL {
            while is_white_space(character) {
                if self.is_js_yaml_4() && character == TAB && self.first_tab_in_line.is_none() {
                    self.first_tab_in_line = Some(self.position);
                }
                character = self.advance();
            }
            if allow_comments && character == HASH {
                loop {
                    character = self.advance();
                    if character == LINE_FEED || character == CARRIAGE_RETURN || character == NUL {
                        break;
                    }
                }
            }
            if !is_eol(character) {
                break;
            }
            self.read_line_break()?;
            character = self.current();
            line_breaks += 1;
            self.line_indent = 0;
            while character == SPACE {
                self.line_indent += 1;
                character = self.advance();
            }
        }
        Ok(line_breaks)
    }

    fn test_document_separator(&self) -> bool {
        let position = self.position;
        let character = self.character_at(position);
        if (character == HYPHEN || character == PERIOD)
            && character == self.character_at(position + 1)
            && character == self.character_at(position + 2)
        {
            let following = self.character_at(position + 3);
            return following == NUL || is_ws_or_eol(following);
        }
        false
    }

    fn write_folded_lines(buffer: &mut Vec<u16>, count: usize) {
        if count == 1 {
            buffer.push(0x20);
        } else if count > 1 {
            buffer.extend(std::iter::repeat_n(0x0A, count - 1));
        }
    }

    fn read_plain_scalar(
        &mut self,
        node_indent: i64,
        within_flow_collection: bool,
    ) -> LoadResult<bool> {
        let mut character = self.current();
        if is_ws_or_eol(character)
            || is_flow_indicator(character)
            || matches!(
                character,
                HASH | AMPERSAND
                    | ASTERISK
                    | EXCLAMATION
                    | VERTICAL_BAR
                    | GREATER_THAN
                    | SINGLE_QUOTE
                    | DOUBLE_QUOTE
                    | PERCENT
                    | AT_SIGN
                    | BACKTICK
            )
        {
            return Ok(false);
        }
        if character == QUESTION_MARK || character == HYPHEN {
            let following = self.character_at(self.position + 1);
            if is_ws_or_eol(following) || (within_flow_collection && is_flow_indicator(following)) {
                return Ok(false);
            }
        }
        let saved_kind = self.kind;
        self.kind = Some(Kind::Scalar);
        let mut buffer: Vec<u16> = Vec::new();
        let mut capture_start = self.position;
        let mut capture_end = self.position;
        let mut has_pending_content = false;
        let mut saved_line = self.line;
        while character != NUL {
            if character == COLON {
                let following = self.character_at(self.position + 1);
                if is_ws_or_eol(following)
                    || (within_flow_collection && is_flow_indicator(following))
                {
                    break;
                }
            } else if character == HASH {
                let preceding = self
                    .position
                    .checked_sub(1)
                    .map_or(NOT_A_CHARACTER, |previous| self.character_at(previous));
                if is_ws_or_eol(preceding) {
                    break;
                }
            } else if (self.position == self.line_start && self.test_document_separator())
                || (within_flow_collection && is_flow_indicator(character))
            {
                break;
            } else if is_eol(character) {
                saved_line = self.line;
                let saved_line_start = self.line_start;
                let saved_line_indent = self.line_indent;
                self.skip_separation_space(false)?;
                if self.line_indent >= node_indent {
                    has_pending_content = true;
                    character = self.current();
                    continue;
                }
                self.position = capture_end;
                self.line = saved_line;
                self.line_start = saved_line_start;
                self.line_indent = saved_line_indent;
                break;
            }
            if has_pending_content {
                self.capture_segment(capture_start, capture_end, false, &mut buffer)?;
                Self::write_folded_lines(&mut buffer, self.line - saved_line);
                capture_start = self.position;
                capture_end = self.position;
                has_pending_content = false;
            }
            if !is_white_space(character) {
                capture_end = self.position + 1;
            }
            character = self.advance();
        }
        self.capture_segment(capture_start, capture_end, false, &mut buffer)?;
        if !buffer.is_empty() {
            self.result = JsValue::String(text_of(&buffer));
            return Ok(true);
        }
        self.kind = saved_kind;
        Ok(false)
    }

    fn read_single_quoted_scalar(&mut self, node_indent: i64) -> LoadResult<bool> {
        let _ = node_indent;
        if self.current() != SINGLE_QUOTE {
            return Ok(false);
        }
        self.kind = Some(Kind::Scalar);
        let mut buffer: Vec<u16> = Vec::new();
        self.position += 1;
        let mut capture_start = self.position;
        let mut capture_end = self.position;
        loop {
            let character = self.current();
            if character == NUL {
                break;
            }
            if character == SINGLE_QUOTE {
                self.capture_segment(capture_start, self.position, true, &mut buffer)?;
                if self.advance() == SINGLE_QUOTE {
                    capture_start = self.position;
                    self.position += 1;
                    capture_end = self.position;
                } else {
                    self.result = JsValue::String(text_of(&buffer));
                    return Ok(true);
                }
            } else if is_eol(character) {
                self.capture_segment(capture_start, capture_end, true, &mut buffer)?;
                let line_breaks = self.skip_separation_space(false)?;
                Self::write_folded_lines(&mut buffer, line_breaks);
                capture_start = self.position;
                capture_end = self.position;
            } else if self.position == self.line_start && self.test_document_separator() {
                return self.fail("unexpected end of the document within a single quoted scalar");
            } else {
                self.position += 1;
                capture_end = self.position;
            }
        }
        self.fail("unexpected end of the stream within a single quoted scalar")
    }

    fn read_double_quoted_scalar(&mut self, node_indent: i64) -> LoadResult<bool> {
        let _ = node_indent;
        if self.current() != DOUBLE_QUOTE {
            return Ok(false);
        }
        self.kind = Some(Kind::Scalar);
        let mut buffer: Vec<u16> = Vec::new();
        self.position += 1;
        let mut capture_start = self.position;
        let mut capture_end = self.position;
        loop {
            let mut character = self.current();
            if character == NUL {
                break;
            }
            if character == DOUBLE_QUOTE {
                self.capture_segment(capture_start, self.position, true, &mut buffer)?;
                self.position += 1;
                self.result = JsValue::String(text_of(&buffer));
                return Ok(true);
            } else if character == BACKSLASH {
                self.capture_segment(capture_start, self.position, true, &mut buffer)?;
                character = self.advance();
                let hex_length = escaped_hex_length(character);
                if is_eol(character) {
                    self.skip_separation_space(false)?;
                } else if let Some(escaped) =
                    simple_escape_sequence(character).filter(|_| character < 256)
                {
                    buffer.push(escaped);
                    self.position += 1;
                } else if hex_length > 0 {
                    let mut code_point: i64 = 0;
                    for _ in 0..hex_length {
                        character = self.advance();
                        let Some(digit) = from_hex_code(character) else {
                            return self.fail("expected hexadecimal character");
                        };
                        code_point = i64::from(to_int32(code_point).wrapping_shl(4)) + digit;
                    }
                    buffer.extend(units_from_code_point(code_point));
                    self.position += 1;
                } else {
                    return self.fail("unknown escape sequence");
                }
                capture_start = self.position;
                capture_end = self.position;
            } else if is_eol(character) {
                self.capture_segment(capture_start, capture_end, true, &mut buffer)?;
                let line_breaks = self.skip_separation_space(false)?;
                Self::write_folded_lines(&mut buffer, line_breaks);
                capture_start = self.position;
                capture_end = self.position;
            } else if self.position == self.line_start && self.test_document_separator() {
                return self.fail("unexpected end of the document within a double quoted scalar");
            } else {
                self.position += 1;
                capture_end = self.position;
            }
        }
        self.fail("unexpected end of the stream within a double quoted scalar")
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a faithful port of js-yaml readFlowCollection keeps its single loop"
    )]
    fn read_flow_collection(&mut self, node_indent: i64) -> LoadResult<bool> {
        let saved_tag = self.tag.clone();
        let saved_anchor = self.anchor.clone();
        let mut character = self.current();
        let (terminator, is_mapping, collection) = if character == LEFT_BRACKET {
            (
                RIGHT_BRACKET,
                false,
                JsValue::Array(self.heap.new_array(Vec::new())),
            )
        } else if character == LEFT_BRACE {
            (RIGHT_BRACE, true, JsValue::Object(self.heap.new_object()))
        } else {
            return Ok(false);
        };
        self.register_current_anchor(collection.clone());
        let mut overridable_keys = HashSet::new();
        let mut read_next = true;
        character = self.advance();
        while character != NUL {
            self.skip_separation_space(true)?;
            character = self.current();
            if character == terminator {
                self.position += 1;
                self.tag = saved_tag;
                self.anchor = saved_anchor;
                self.kind = Some(if is_mapping {
                    Kind::Mapping
                } else {
                    Kind::Sequence
                });
                self.result = collection;
                return Ok(true);
            } else if !read_next {
                return self.fail("missed comma between flow collection entries");
            } else if self.is_js_yaml_4() && character == COMMA {
                return self.fail("expected the node content, but found ','");
            }
            let mut value_node = JsValue::Null;
            let mut is_pair = false;
            let mut is_explicit_pair = false;
            if character == QUESTION_MARK && is_ws_or_eol(self.character_at(self.position + 1)) {
                is_pair = true;
                is_explicit_pair = true;
                self.position += 1;
                self.skip_separation_space(true)?;
            }
            let line = self.line;
            let key_start = if self.is_js_yaml_4() {
                KeyStart {
                    line: Some(line),
                    line_start: Some(self.line_start),
                    position: Some(self.position),
                }
            } else {
                KeyStart::default()
            };
            self.compose_node(node_indent, NodeContext::FlowIn, false, true)?;
            let key_tag = self.tag.clone();
            let key_node = self.result.clone();
            self.skip_separation_space(true)?;
            character = self.current();
            if (is_explicit_pair || self.line == line) && character == COLON {
                is_pair = true;
                self.advance();
                self.skip_separation_space(true)?;
                self.compose_node(node_indent, NodeContext::FlowIn, false, true)?;
                value_node = self.result.clone();
            }
            match &collection {
                JsValue::Object(id) => {
                    self.store_mapping_pair(
                        Some(*id),
                        &mut overridable_keys,
                        key_tag.as_deref(),
                        &key_node,
                        value_node,
                        key_start,
                    )?;
                }
                JsValue::Array(id) => {
                    let item = if is_pair {
                        JsValue::Object(self.store_mapping_pair(
                            None,
                            &mut overridable_keys,
                            key_tag.as_deref(),
                            &key_node,
                            value_node,
                            key_start,
                        )?)
                    } else {
                        key_node
                    };
                    self.heap.arrays[*id].push(item);
                }
                _ => {}
            }
            self.skip_separation_space(true)?;
            character = self.current();
            if character == COMMA {
                read_next = true;
                character = self.advance();
            } else {
                read_next = false;
            }
        }
        self.fail("unexpected end of the stream within a flow collection")
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a faithful port of js-yaml readBlockScalar keeps its header and body loops together"
    )]
    fn read_block_scalar(&mut self, node_indent: i64) -> LoadResult<bool> {
        let mut character = self.current();
        let folding = if character == VERTICAL_BAR {
            false
        } else if character == GREATER_THAN {
            true
        } else {
            return Ok(false);
        };
        self.kind = Some(Kind::Scalar);
        let mut buffer: Vec<u16> = Vec::new();
        let mut chomping = Chomping::Clip;
        let mut did_read_content = false;
        let mut detected_indent = false;
        let mut text_indent = node_indent;
        let mut empty_lines = 0usize;
        let mut at_more_indented = false;
        while character != NUL {
            character = self.advance();
            if character == PLUS || character == HYPHEN {
                if chomping == Chomping::Clip {
                    chomping = if character == PLUS {
                        Chomping::Keep
                    } else {
                        Chomping::Strip
                    };
                } else {
                    return self.fail("repeat of a chomping mode identifier");
                }
            } else if let Some(width) =
                char::from_u32(character).and_then(|digit| digit.to_digit(10))
            {
                if width == 0 {
                    return self.fail(
                        "bad explicit indentation width of a block scalar; it cannot be less than one",
                    );
                } else if detected_indent {
                    return self.fail("repeat of an indentation width identifier");
                }
                text_indent = node_indent + i64::from(width) - 1;
                detected_indent = true;
            } else {
                break;
            }
        }
        if is_white_space(character) {
            loop {
                character = self.advance();
                if !is_white_space(character) {
                    break;
                }
            }
            if character == HASH {
                loop {
                    character = self.advance();
                    if is_eol(character) || character == NUL {
                        break;
                    }
                }
            }
        }
        let line_feeds = |count: usize| std::iter::repeat_n(0x0A_u16, count);
        while character != NUL {
            self.read_line_break()?;
            self.line_indent = 0;
            character = self.current();
            while (!detected_indent || self.line_indent < text_indent) && character == SPACE {
                self.line_indent += 1;
                character = self.advance();
            }
            if !detected_indent && self.line_indent > text_indent {
                text_indent = self.line_indent;
            }
            if is_eol(character) {
                empty_lines += 1;
                continue;
            }
            if self.line_indent < text_indent {
                match chomping {
                    Chomping::Keep => buffer.extend(line_feeds(if did_read_content {
                        1 + empty_lines
                    } else {
                        empty_lines
                    })),
                    Chomping::Clip => {
                        if did_read_content {
                            buffer.push(0x0A);
                        }
                    }
                    Chomping::Strip => {}
                }
                break;
            }
            if folding {
                if is_white_space(character) {
                    at_more_indented = true;
                    buffer.extend(line_feeds(if did_read_content {
                        1 + empty_lines
                    } else {
                        empty_lines
                    }));
                } else if at_more_indented {
                    at_more_indented = false;
                    buffer.extend(line_feeds(empty_lines + 1));
                } else if empty_lines == 0 {
                    if did_read_content {
                        buffer.push(0x20);
                    }
                } else {
                    buffer.extend(line_feeds(empty_lines));
                }
            } else {
                buffer.extend(line_feeds(if did_read_content {
                    1 + empty_lines
                } else {
                    empty_lines
                }));
            }
            did_read_content = true;
            detected_indent = true;
            empty_lines = 0;
            let capture_start = self.position;
            while !is_eol(character) && character != NUL {
                character = self.advance();
            }
            self.capture_segment(capture_start, self.position, false, &mut buffer)?;
        }
        self.result = JsValue::String(text_of(&buffer));
        Ok(true)
    }

    fn fail_on_tab_indentation(&mut self) -> LoadResult<()> {
        if let Some(tab_position) = self.first_tab_in_line {
            self.position = tab_position;
            return self.fail("tab characters must not be used in indentation");
        }
        Ok(())
    }

    fn read_block_sequence(&mut self, node_indent: i64) -> LoadResult<bool> {
        if self.is_js_yaml_4() && self.first_tab_in_line.is_some() {
            return Ok(false);
        }
        let saved_tag = self.tag.clone();
        let saved_anchor = self.anchor.clone();
        let sequence = self.heap.new_array(Vec::new());
        self.register_current_anchor(JsValue::Array(sequence));
        let mut detected = false;
        let mut character = self.current();
        while character != NUL {
            if self.is_js_yaml_4() {
                self.fail_on_tab_indentation()?;
            }
            if character != HYPHEN || !is_ws_or_eol(self.character_at(self.position + 1)) {
                break;
            }
            detected = true;
            self.position += 1;
            if self.skip_separation_space(true)? > 0 && self.line_indent <= node_indent {
                self.heap.arrays[sequence].push(JsValue::Null);
                character = self.current();
                continue;
            }
            let line = self.line;
            self.compose_node(node_indent, NodeContext::BlockIn, false, true)?;
            let item = self.result.clone();
            self.heap.arrays[sequence].push(item);
            self.skip_separation_space(true)?;
            character = self.current();
            if (self.line == line || self.line_indent > node_indent) && character != NUL {
                return self.fail("bad indentation of a sequence entry");
            } else if self.line_indent < node_indent {
                break;
            }
        }
        if detected {
            self.tag = saved_tag;
            self.anchor = saved_anchor;
            self.kind = Some(Kind::Sequence);
            self.result = JsValue::Array(sequence);
        }
        Ok(detected)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a faithful port of js-yaml readBlockMapping keeps its single state machine"
    )]
    fn read_block_mapping(&mut self, node_indent: i64, flow_indent: i64) -> LoadResult<bool> {
        if self.is_js_yaml_4() && self.first_tab_in_line.is_some() {
            return Ok(false);
        }
        let saved_tag = self.tag.clone();
        let saved_anchor = self.anchor.clone();
        let mapping = self.heap.new_object();
        self.register_current_anchor(JsValue::Object(mapping));
        let mut overridable_keys = HashSet::new();
        let mut key_tag: Option<String> = None;
        let mut key_node = JsValue::Null;
        let mut value_node = JsValue::Null;
        let mut at_explicit_key = false;
        let mut detected = false;
        let mut allow_compact: bool;
        let mut key_start = KeyStart::default();
        let explicit_key_start = |loader: &Self, js_yaml_4_start: KeyStart| {
            if loader.is_js_yaml_4() {
                js_yaml_4_start
            } else {
                KeyStart::default()
            }
        };
        let mut character = self.current();
        while character != NUL {
            if self.is_js_yaml_4() && !at_explicit_key {
                self.fail_on_tab_indentation()?;
            }
            let following = self.character_at(self.position + 1);
            let line = self.line;
            let entry_position = self.position;
            if (character == QUESTION_MARK || character == COLON) && is_ws_or_eol(following) {
                if character == QUESTION_MARK {
                    if at_explicit_key {
                        let start = explicit_key_start(self, key_start);
                        self.store_mapping_pair(
                            Some(mapping),
                            &mut overridable_keys,
                            key_tag.as_deref(),
                            &key_node,
                            JsValue::Null,
                            start,
                        )?;
                        key_tag = None;
                        key_node = JsValue::Null;
                        value_node = JsValue::Null;
                    }
                    detected = true;
                    at_explicit_key = true;
                    allow_compact = true;
                } else if at_explicit_key {
                    at_explicit_key = false;
                    allow_compact = true;
                } else {
                    return self.fail("incomplete explicit mapping pair; a key node is missed; or followed by a non-tabulated empty line");
                }
                self.position += 1;
                character = following;
            } else {
                if self.is_js_yaml_4() {
                    key_start = KeyStart {
                        line: Some(self.line),
                        line_start: Some(self.line_start),
                        position: Some(self.position),
                    };
                }
                if !self.compose_node(flow_indent, NodeContext::FlowOut, false, true)? {
                    break;
                }
                if self.line == line {
                    character = self.current();
                    while is_white_space(character) {
                        character = self.advance();
                    }
                    if character == COLON {
                        character = self.advance();
                        if !is_ws_or_eol(character) {
                            return self.fail("a whitespace character is expected after the key-value separator within a block mapping");
                        }
                        if at_explicit_key {
                            let start = explicit_key_start(self, key_start);
                            self.store_mapping_pair(
                                Some(mapping),
                                &mut overridable_keys,
                                key_tag.as_deref(),
                                &key_node,
                                JsValue::Null,
                                start,
                            )?;
                            value_node = JsValue::Null;
                        }
                        detected = true;
                        at_explicit_key = false;
                        allow_compact = false;
                        key_tag = self.tag.clone();
                        key_node = self.result.clone();
                    } else if detected {
                        return self
                            .fail("can not read an implicit mapping pair; a colon is missed");
                    } else {
                        self.tag = saved_tag;
                        self.anchor = saved_anchor;
                        return Ok(true);
                    }
                } else if detected {
                    return self.fail("can not read a block mapping entry; a multiline key may not be an implicit key");
                } else {
                    self.tag = saved_tag;
                    self.anchor = saved_anchor;
                    return Ok(true);
                }
            }
            if self.line == line || self.line_indent > node_indent {
                if self.is_js_yaml_4() && at_explicit_key {
                    key_start = KeyStart {
                        line: Some(self.line),
                        line_start: Some(self.line_start),
                        position: Some(self.position),
                    };
                }
                if self.compose_node(node_indent, NodeContext::BlockOut, true, allow_compact)? {
                    if at_explicit_key {
                        key_node = self.result.clone();
                    } else {
                        value_node = self.result.clone();
                    }
                }
                if !at_explicit_key {
                    let start = if self.is_js_yaml_4() {
                        key_start
                    } else {
                        KeyStart {
                            line: Some(line),
                            line_start: None,
                            position: Some(entry_position),
                        }
                    };
                    self.store_mapping_pair(
                        Some(mapping),
                        &mut overridable_keys,
                        key_tag.as_deref(),
                        &key_node,
                        std::mem::replace(&mut value_node, JsValue::Null),
                        start,
                    )?;
                    key_tag = None;
                    key_node = JsValue::Null;
                }
                self.skip_separation_space(true)?;
                character = self.current();
            }
            let is_misindented = if self.is_js_yaml_4() {
                self.line == line || self.line_indent > node_indent
            } else {
                self.line_indent > node_indent
            };
            if is_misindented && character != NUL {
                return self.fail("bad indentation of a mapping entry");
            } else if self.line_indent < node_indent {
                break;
            }
        }
        if at_explicit_key {
            let start = explicit_key_start(self, key_start);
            self.store_mapping_pair(
                Some(mapping),
                &mut overridable_keys,
                key_tag.as_deref(),
                &key_node,
                JsValue::Null,
                start,
            )?;
        }
        if detected {
            self.tag = saved_tag;
            self.anchor = saved_anchor;
            self.kind = Some(Kind::Mapping);
            self.result = JsValue::Object(mapping);
        }
        Ok(detected)
    }

    fn read_tag_property(&mut self) -> LoadResult<bool> {
        let mut character = self.current();
        if character != EXCLAMATION {
            return Ok(false);
        }
        if self.tag.is_some() {
            return self.fail("duplication of a tag property");
        }
        character = self.advance();
        let mut is_verbatim = false;
        let mut is_named = false;
        let mut tag_handle: Vec<u16> = vec![0x21];
        if character == LESS_THAN {
            is_verbatim = true;
            character = self.advance();
        } else if character == EXCLAMATION {
            is_named = true;
            tag_handle = vec![0x21, 0x21];
            character = self.advance();
        }
        let mut start = self.position;
        let tag_name: Vec<u16>;
        if is_verbatim {
            loop {
                character = self.advance();
                if character == NUL || character == GREATER_THAN {
                    break;
                }
            }
            if self.position < self.length {
                tag_name = self.slice(start, self.position);
                self.advance();
            } else {
                return self.fail("unexpected end of the stream within a verbatim tag");
            }
        } else {
            while character != NUL && !is_ws_or_eol(character) {
                if character == EXCLAMATION {
                    if is_named {
                        return self.fail("tag suffix cannot contain exclamation marks");
                    }
                    tag_handle = self.slice(start.saturating_sub(1), self.position + 1);
                    if !is_tag_handle(&tag_handle) {
                        return self.fail("named tag handle cannot contain such characters");
                    }
                    is_named = true;
                    start = self.position + 1;
                }
                character = self.advance();
            }
            tag_name = self.slice(start, self.position);
            if tag_name
                .iter()
                .any(|unit| is_flow_indicator(u32::from(*unit)))
            {
                return self.fail("tag suffix cannot contain flow indicator characters");
            }
        }
        if !tag_name.is_empty() && !is_tag_uri(&tag_name) {
            return self.fail(format!(
                "tag name cannot contain such characters: {}",
                text_of(&tag_name)
            ));
        }
        let tag_name = if self.is_js_yaml_4() {
            match decode_uri_component(&tag_name) {
                Some(decoded) => decoded,
                None => {
                    return self.fail(format!("tag name is malformed: {}", text_of(&tag_name)));
                }
            }
        } else {
            tag_name
        };
        let tag_name = text_of(&tag_name);
        let tag_handle = text_of(&tag_handle);
        self.tag = Some(if is_verbatim {
            tag_name
        } else if let Some(prefix) = self.tag_map.get(&tag_handle) {
            format!("{prefix}{tag_name}")
        } else if tag_handle == "!" {
            format!("!{tag_name}")
        } else if tag_handle == "!!" {
            format!("{CORE_TAG_PREFIX}{tag_name}")
        } else {
            return self.fail(format!("undeclared tag handle \"{tag_handle}\""));
        });
        Ok(true)
    }

    fn read_anchor_property(&mut self) -> LoadResult<bool> {
        let mut character = self.current();
        if character != AMPERSAND {
            return Ok(false);
        }
        if self.anchor.is_some() {
            return self.fail("duplication of an anchor property");
        }
        character = self.advance();
        let start = self.position;
        while character != NUL && !is_ws_or_eol(character) && !is_flow_indicator(character) {
            character = self.advance();
        }
        if self.position == start {
            return self.fail("name of an anchor node must contain at least one character");
        }
        self.anchor = Some(text_of(&self.slice(start, self.position)));
        Ok(true)
    }

    fn read_alias(&mut self) -> LoadResult<bool> {
        let mut character = self.current();
        if character != ASTERISK {
            return Ok(false);
        }
        character = self.advance();
        let start = self.position;
        while character != NUL && !is_ws_or_eol(character) && !is_flow_indicator(character) {
            character = self.advance();
        }
        if self.position == start {
            return self.fail("name of an alias node must contain at least one character");
        }
        let alias = text_of(&self.slice(start, self.position));
        let Some(value) = self.anchor_map.get(&alias).cloned() else {
            return self.fail(format!("unidentified alias \"{alias}\""));
        };
        self.result = value;
        self.skip_separation_space(true)?;
        Ok(true)
    }

    fn indent_status(&self, parent_indent: i64) -> IndentStatus {
        match self.line_indent.cmp(&parent_indent) {
            std::cmp::Ordering::Greater => IndentStatus::Deeper,
            std::cmp::Ordering::Equal => IndentStatus::Same,
            std::cmp::Ordering::Less => IndentStatus::Shallower,
        }
    }

    fn compose_node(
        &mut self,
        parent_indent: i64,
        node_context: NodeContext,
        allow_to_seek: bool,
        allow_compact: bool,
    ) -> LoadResult<bool> {
        self.nesting_depth += 1;
        if self.nesting_depth > MAXIMUM_NESTING_DEPTH {
            return Err(LoadError::Thrown {
                name: "RangeError",
                message: "Maximum call stack size exceeded".to_string(),
            });
        }
        let composed =
            self.compose_node_at_depth(parent_indent, node_context, allow_to_seek, allow_compact);
        self.nesting_depth -= 1;
        composed
    }

    fn compose_node_at_depth(
        &mut self,
        parent_indent: i64,
        node_context: NodeContext,
        allow_to_seek: bool,
        allow_compact: bool,
    ) -> LoadResult<bool> {
        self.tag = None;
        self.anchor = None;
        self.kind = None;
        self.result = JsValue::Null;
        let allow_block_styles =
            matches!(node_context, NodeContext::BlockOut | NodeContext::BlockIn);
        let allow_block_scalars = allow_block_styles;
        let mut allow_block_collections = allow_block_styles;
        let mut indent_status = IndentStatus::Deeper;
        let mut at_new_line = false;
        let mut has_content = false;
        if allow_to_seek && self.skip_separation_space(true)? > 0 {
            at_new_line = true;
            indent_status = self.indent_status(parent_indent);
        }
        if indent_status == IndentStatus::Deeper {
            while self.read_tag_property()? || self.read_anchor_property()? {
                if self.skip_separation_space(true)? > 0 {
                    at_new_line = true;
                    allow_block_collections = allow_block_styles;
                    indent_status = self.indent_status(parent_indent);
                } else {
                    allow_block_collections = false;
                }
            }
        }
        if allow_block_collections {
            allow_block_collections = at_new_line || allow_compact;
        }
        if indent_status == IndentStatus::Deeper || node_context == NodeContext::BlockOut {
            let flow_indent = if matches!(node_context, NodeContext::FlowIn | NodeContext::FlowOut)
            {
                parent_indent
            } else {
                parent_indent + 1
            };
            let block_indent = to_i64(self.position) - to_i64(self.line_start);
            if indent_status == IndentStatus::Deeper {
                if (allow_block_collections
                    && (self.read_block_sequence(block_indent)?
                        || self.read_block_mapping(block_indent, flow_indent)?))
                    || self.read_flow_collection(flow_indent)?
                {
                    has_content = true;
                } else {
                    if (allow_block_scalars && self.read_block_scalar(flow_indent)?)
                        || self.read_single_quoted_scalar(flow_indent)?
                        || self.read_double_quoted_scalar(flow_indent)?
                    {
                        has_content = true;
                    } else if self.read_alias()? {
                        has_content = true;
                        if self.tag.is_some() || self.anchor.is_some() {
                            return self.fail("alias node should not have any properties");
                        }
                    } else if self
                        .read_plain_scalar(flow_indent, node_context == NodeContext::FlowIn)?
                    {
                        has_content = true;
                        if self.tag.is_none() {
                            self.tag = Some("?".to_string());
                        }
                    }
                    self.register_current_anchor(self.result.clone());
                }
            } else if indent_status == IndentStatus::Same {
                has_content = allow_block_collections && self.read_block_sequence(block_indent)?;
            }
        }
        self.resolve_tag()?;
        Ok(self.tag.is_some() || self.anchor.is_some() || has_content)
    }

    fn type_error(message: String) -> LoadError {
        LoadError::Thrown {
            name: "TypeError",
            message,
        }
    }

    fn resolve_tag(&mut self) -> LoadResult<()> {
        let Some(tag) = self.tag.clone() else {
            if self.is_js_yaml_4() {
                self.register_current_anchor(self.result.clone());
            }
            return Ok(());
        };
        if tag == "!" {
            return Ok(());
        }
        let dialect = self.dialect;
        if tag == "?" {
            if !self.result.is_null() && self.kind != Some(Kind::Scalar) {
                return self.fail(format!(
                    "unacceptable node kind for !<?> tag; it should be \"scalar\", not \"{}\"",
                    kind_name(self.kind)
                ));
            }
            for implicit_type in IMPLICIT_TYPES {
                let mut types = TypeContext {
                    heap: &mut self.heap,
                    dialect,
                };
                if types
                    .resolve(implicit_type, &self.result)
                    .map_err(Self::type_error)?
                {
                    let constructed = types
                        .construct(implicit_type, self.result.clone())
                        .map_err(Self::type_error)?;
                    self.result = constructed;
                    self.tag = Some(implicit_type.tag().to_string());
                    self.register_current_anchor(self.result.clone());
                    break;
                }
            }
            return Ok(());
        }
        let Some(explicit_type) = YamlType::find(&tag, self.kind) else {
            return self.fail(format!("unknown tag !<{tag}>"));
        };
        if !self.result.is_null() && Some(explicit_type.kind()) != self.kind {
            return self.fail(format!(
                "unacceptable node kind for !<{tag}> tag; it should be \"{}\", not \"{}\"",
                kind_name(Some(explicit_type.kind())),
                kind_name(self.kind)
            ));
        }
        let mut types = TypeContext {
            heap: &mut self.heap,
            dialect,
        };
        if !types
            .resolve(explicit_type, &self.result)
            .map_err(Self::type_error)?
        {
            return self.fail(format!("cannot resolve a node with !<{tag}> explicit tag"));
        }
        let constructed = types
            .construct(explicit_type, self.result.clone())
            .map_err(Self::type_error)?;
        self.result = constructed;
        self.register_current_anchor(self.result.clone());
        Ok(())
    }

    fn handle_yaml_directive(&mut self, arguments: &[Vec<u16>]) -> LoadResult<()> {
        if self.version.is_some() {
            return self.fail("duplication of %YAML directive");
        }
        let [argument] = arguments else {
            return self.fail("YAML directive accepts exactly one argument");
        };
        let text = text_of(argument);
        let Some((major, minor)) = text.split_once('.').filter(|(major, minor)| {
            !major.is_empty()
                && !minor.is_empty()
                && major.bytes().all(|byte| byte.is_ascii_digit())
                && minor.bytes().all(|byte| byte.is_ascii_digit())
        }) else {
            return self.fail("ill-formed argument of the YAML directive");
        };
        let _ = minor;
        if major.parse::<f64>().ok() != Some(1.0) {
            return self.fail("unacceptable YAML version of the document");
        }
        self.version = Some(text);
        Ok(())
    }

    fn handle_tag_directive(&mut self, arguments: &[Vec<u16>]) -> LoadResult<()> {
        let [handle, prefix] = arguments else {
            return self.fail("TAG directive accepts exactly two arguments");
        };
        if !is_tag_handle(handle) {
            return self.fail("ill-formed tag handle (first argument) of the TAG directive");
        }
        let handle_text = text_of(handle);
        if self.tag_map.contains_key(&handle_text) {
            return self.fail(format!(
                "there is a previously declared suffix for \"{handle_text}\" tag handle"
            ));
        }
        if !is_tag_uri(prefix) {
            return self.fail("ill-formed tag prefix (second argument) of the TAG directive");
        }
        let prefix = if self.is_js_yaml_4() {
            match decode_uri_component(prefix) {
                Some(decoded) => decoded,
                None => {
                    return self.fail(format!("tag prefix is malformed: {}", text_of(prefix)));
                }
            }
        } else {
            prefix.clone()
        };
        self.tag_map.insert(handle_text, text_of(&prefix));
        Ok(())
    }

    fn read_document(&mut self) -> LoadResult<()> {
        self.version = None;
        self.tag_map.clear();
        self.anchor_map.clear();
        let mut has_directives = false;
        while self.current() != NUL {
            self.skip_separation_space(true)?;
            let mut character = self.current();
            if self.line_indent > 0 || character != PERCENT {
                break;
            }
            has_directives = true;
            character = self.advance();
            let mut start = self.position;
            while character != NUL && !is_ws_or_eol(character) {
                character = self.advance();
            }
            let directive_name = self.slice(start, self.position);
            let mut directive_arguments: Vec<Vec<u16>> = Vec::new();
            if directive_name.is_empty() {
                return self.fail("directive name must not be less than one character in length");
            }
            while character != NUL {
                while is_white_space(character) {
                    character = self.advance();
                }
                if character == HASH {
                    loop {
                        character = self.advance();
                        if character == NUL || is_eol(character) {
                            break;
                        }
                    }
                    break;
                }
                if is_eol(character) {
                    break;
                }
                start = self.position;
                while character != NUL && !is_ws_or_eol(character) {
                    character = self.advance();
                }
                directive_arguments.push(self.slice(start, self.position));
            }
            if character != NUL {
                self.read_line_break()?;
            }
            match text_of(&directive_name).as_str() {
                "YAML" => self.handle_yaml_directive(&directive_arguments)?,
                "TAG" => self.handle_tag_directive(&directive_arguments)?,
                _ => {}
            }
        }
        self.skip_separation_space(true)?;
        if self.line_indent == 0
            && self.current() == HYPHEN
            && self.character_at(self.position + 1) == HYPHEN
            && self.character_at(self.position + 2) == HYPHEN
        {
            self.position += 3;
            self.skip_separation_space(true)?;
        } else if has_directives {
            return self.fail("directives end mark is expected");
        }
        self.compose_node(self.line_indent - 1, NodeContext::BlockOut, false, true)?;
        self.skip_separation_space(true)?;
        self.documents.push(self.result.clone());
        if self.position == self.line_start && self.test_document_separator() {
            if self.current() == PERIOD {
                self.position += 3;
                self.skip_separation_space(true)?;
            }
            return Ok(());
        }
        if self.position + 1 < self.length {
            return self.fail("end of the stream or a document separator is expected");
        }
        Ok(())
    }
}
