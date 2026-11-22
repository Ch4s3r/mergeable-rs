use super::Dialect;
use super::loader::Kind;
use super::value::{Heap, JsValue, days_from_civil};
use regex::Regex;
use std::sync::LazyLock;

const BASE64_ALPHABET: &[u8; 67] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=\n\r";
const BASE64_PADDING_CODE: usize = 64;
const MAXIMUM_TIME_VALUE: f64 = 8.64e15;

static JS_YAML_3_FLOAT_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:[-+]?(?:0|[1-9][0-9_]*)(?:\.[0-9_]*)?(?:[eE][-+]?[0-9]+)?",
        r"|\.[0-9_]+(?:[eE][-+]?[0-9]+)?",
        r"|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*",
        r"|[-+]?\.(?:inf|Inf|INF)",
        r"|\.(?:nan|NaN|NAN))$"
    ))
    .ok()
});

static JS_YAML_4_FLOAT_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:[-+]?(?:[0-9][0-9_]*)(?:\.[0-9_]*)?(?:[eE][-+]?[0-9]+)?",
        r"|\.[0-9_]+(?:[eE][-+]?[0-9]+)?",
        r"|[-+]?\.(?:inf|Inf|INF)",
        r"|\.(?:nan|NaN|NAN))$"
    ))
    .ok()
});

static SEXAGESIMAL_TAIL_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(:[0-5]?[0-9])+$").ok());

static DATE_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^([0-9][0-9][0-9][0-9])-([0-9][0-9])-([0-9][0-9])$").ok());

static TIMESTAMP_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^([0-9][0-9][0-9][0-9])",
        r"-([0-9][0-9]?)",
        r"-([0-9][0-9]?)",
        r"(?:[Tt]|[ \t]+)",
        r"([0-9][0-9]?)",
        r":([0-9][0-9])",
        r":([0-9][0-9])",
        r"(?:\.([0-9]*))?",
        r"(?:[ \t]*(Z|([-+])([0-9][0-9]?)",
        r"(?::([0-9][0-9]))?))?$"
    ))
    .ok()
});

fn matches(pattern: &LazyLock<Option<Regex>>, text: &str) -> bool {
    pattern
        .as_ref()
        .is_some_and(|compiled| compiled.is_match(text))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum YamlType {
    Null,
    Bool,
    Int,
    Float,
    Timestamp,
    Merge,
    Str,
    Seq,
    Map,
    Binary,
    Omap,
    Pairs,
    Set,
}

pub(super) const IMPLICIT_TYPES: [YamlType; 6] = [
    YamlType::Null,
    YamlType::Bool,
    YamlType::Int,
    YamlType::Float,
    YamlType::Timestamp,
    YamlType::Merge,
];

const ALL_TYPES: [YamlType; 13] = [
    YamlType::Str,
    YamlType::Seq,
    YamlType::Map,
    YamlType::Null,
    YamlType::Bool,
    YamlType::Int,
    YamlType::Float,
    YamlType::Timestamp,
    YamlType::Merge,
    YamlType::Binary,
    YamlType::Omap,
    YamlType::Pairs,
    YamlType::Set,
];

pub(super) const MERGE_TAG: &str = "tag:yaml.org,2002:merge";

impl YamlType {
    pub(super) fn tag(self) -> &'static str {
        match self {
            Self::Null => "tag:yaml.org,2002:null",
            Self::Bool => "tag:yaml.org,2002:bool",
            Self::Int => "tag:yaml.org,2002:int",
            Self::Float => "tag:yaml.org,2002:float",
            Self::Timestamp => "tag:yaml.org,2002:timestamp",
            Self::Merge => MERGE_TAG,
            Self::Str => "tag:yaml.org,2002:str",
            Self::Seq => "tag:yaml.org,2002:seq",
            Self::Map => "tag:yaml.org,2002:map",
            Self::Binary => "tag:yaml.org,2002:binary",
            Self::Omap => "tag:yaml.org,2002:omap",
            Self::Pairs => "tag:yaml.org,2002:pairs",
            Self::Set => "tag:yaml.org,2002:set",
        }
    }

    pub(super) fn kind(self) -> Kind {
        match self {
            Self::Seq | Self::Omap | Self::Pairs => Kind::Sequence,
            Self::Map | Self::Set => Kind::Mapping,
            _ => Kind::Scalar,
        }
    }

    pub(super) fn find(tag: &str, kind: Option<Kind>) -> Option<Self> {
        ALL_TYPES.into_iter().find(|candidate| {
            candidate.tag() == tag && kind.is_none_or(|node_kind| candidate.kind() == node_kind)
        })
    }
}

pub(super) struct TypeContext<'heap> {
    pub(super) heap: &'heap mut Heap,
    pub(super) dialect: Dialect,
}

fn type_error(message: &str) -> String {
    message.to_string()
}

impl TypeContext<'_> {
    fn coerce_to_string(&self, data: &JsValue) -> String {
        self.heap.to_js_string(data, self.dialect)
    }

    pub(super) fn resolve(&self, yaml_type: YamlType, data: &JsValue) -> Result<bool, String> {
        match yaml_type {
            YamlType::Null => Ok(match data {
                JsValue::Null => true,
                JsValue::String(text) => matches!(text.as_str(), "~" | "null" | "Null" | "NULL"),
                _ => false,
            }),
            YamlType::Bool => Ok(matches!(
                data,
                JsValue::String(text)
                    if matches!(text.as_str(), "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
            )),
            YamlType::Int => match data {
                JsValue::String(text) => Ok(resolve_integer(text.as_bytes(), self.dialect)),
                JsValue::Binary(bytes) if !bytes.is_empty() => {
                    Err(type_error("data.charCodeAt is not a function"))
                }
                _ => Ok(false),
            },
            YamlType::Float => {
                if data.is_null() {
                    return Ok(false);
                }
                let pattern = match self.dialect {
                    Dialect::JsYaml3 => &JS_YAML_3_FLOAT_PATTERN,
                    Dialect::JsYaml4 => &JS_YAML_4_FLOAT_PATTERN,
                };
                let ends_with_underscore =
                    matches!(data, JsValue::String(text) if text.ends_with('_'));
                Ok(matches(pattern, &self.coerce_to_string(data)) && !ends_with_underscore)
            }
            YamlType::Timestamp => {
                if data.is_null() {
                    return Ok(false);
                }
                let text = self.coerce_to_string(data);
                Ok(matches(&DATE_PATTERN, &text) || matches(&TIMESTAMP_PATTERN, &text))
            }
            YamlType::Merge => Ok(match data {
                JsValue::Null => true,
                JsValue::String(text) => text == "<<",
                _ => false,
            }),
            YamlType::Binary => match data {
                JsValue::Null => Ok(false),
                JsValue::String(text) => Ok(resolve_binary(text)),
                JsValue::Binary(bytes) if !bytes.is_empty() => {
                    Err(type_error("data.charAt is not a function"))
                }
                _ => Ok(true),
            },
            YamlType::Str | YamlType::Seq | YamlType::Map => Ok(true),
            YamlType::Omap => Ok(self.resolve_ordered_map(data)),
            YamlType::Pairs => Ok(self.resolve_pairs(data)),
            YamlType::Set => Ok(match data {
                JsValue::Object(id) => self.heap.objects[*id].values().all(JsValue::is_null),
                _ => true,
            }),
        }
    }

    fn single_key_pairs(&self, data: &JsValue) -> Option<Vec<Option<String>>> {
        let JsValue::Array(id) = data else {
            return None;
        };
        Some(
            self.heap.arrays[*id]
                .iter()
                .map(|pair| match pair {
                    JsValue::Object(pair_id) => {
                        let pair_object = &self.heap.objects[*pair_id];
                        if pair_object.len() == 1 {
                            pair_object
                                .entries_in_enumeration_order()
                                .into_iter()
                                .next()
                                .map(|(key, _)| key)
                        } else {
                            None
                        }
                    }
                    _ => None,
                })
                .collect(),
        )
    }

    fn resolve_ordered_map(&self, data: &JsValue) -> bool {
        if data.is_null() {
            return true;
        }
        let Some(pair_keys) = self.single_key_pairs(data) else {
            return true;
        };
        let mut seen_keys: Vec<String> = Vec::new();
        for pair_key in pair_keys {
            let Some(key) = pair_key else {
                return false;
            };
            if seen_keys.contains(&key) {
                return false;
            }
            seen_keys.push(key);
        }
        true
    }

    fn resolve_pairs(&self, data: &JsValue) -> bool {
        if data.is_null() {
            return true;
        }
        self.single_key_pairs(data)
            .is_none_or(|pair_keys| pair_keys.iter().all(Option::is_some))
    }

    pub(super) fn construct(
        &mut self,
        yaml_type: YamlType,
        data: JsValue,
    ) -> Result<JsValue, String> {
        match yaml_type {
            YamlType::Null => Ok(JsValue::Null),
            YamlType::Bool => Ok(JsValue::Bool(matches!(
                &data,
                JsValue::String(text) if matches!(text.as_str(), "true" | "True" | "TRUE")
            ))),
            YamlType::Int => match &data {
                JsValue::String(text) => Ok(JsValue::Number(construct_integer(text, self.dialect))),
                _ => Err(type_error("value.indexOf is not a function")),
            },
            YamlType::Float => match &data {
                JsValue::String(text) => Ok(JsValue::Number(construct_float(text, self.dialect))),
                _ => Err(type_error("data.replace is not a function")),
            },
            YamlType::Timestamp => {
                let text = self.coerce_to_string(&data);
                construct_timestamp(&text)
                    .map(JsValue::Date)
                    .ok_or_else(|| type_error("Date resolve error"))
            }
            YamlType::Merge => Ok(data),
            YamlType::Str => Ok(match data {
                JsValue::Null => JsValue::String(String::new()),
                other => other,
            }),
            YamlType::Seq | YamlType::Omap => Ok(match data {
                JsValue::Null => JsValue::Array(self.heap.new_array(Vec::new())),
                other => other,
            }),
            YamlType::Map | YamlType::Set => Ok(match data {
                JsValue::Null => JsValue::Object(self.heap.new_object()),
                other => other,
            }),
            YamlType::Binary => match &data {
                JsValue::String(text) => Ok(JsValue::Binary(construct_binary(text))),
                _ => Err(type_error("data.replace is not a function")),
            },
            YamlType::Pairs => Ok(self.construct_pairs(&data)),
        }
    }

    fn construct_pairs(&mut self, data: &JsValue) -> JsValue {
        let JsValue::Array(id) = data else {
            return JsValue::Array(self.heap.new_array(Vec::new()));
        };
        let pairs: Vec<(String, JsValue)> = self.heap.arrays[*id]
            .iter()
            .filter_map(|pair| match pair {
                JsValue::Object(pair_id) => self.heap.objects[*pair_id]
                    .entries_in_enumeration_order()
                    .into_iter()
                    .next(),
                _ => None,
            })
            .collect();
        let pair_arrays = pairs
            .into_iter()
            .map(|(key, value)| {
                JsValue::Array(self.heap.new_array(vec![JsValue::String(key), value]))
            })
            .collect();
        JsValue::Array(self.heap.new_array(pair_arrays))
    }
}

fn resolve_integer(text: &[u8], dialect: Dialect) -> bool {
    let length = text.len();
    if length == 0 {
        return false;
    }
    let mut index = 0;
    let mut character = text.first().copied();
    if matches!(character, Some(b'-' | b'+')) {
        index += 1;
        character = text.get(index).copied();
    }
    if character == Some(b'0') {
        if index + 1 == length {
            return true;
        }
        index += 1;
        character = text.get(index).copied();
        let radix_digits: Option<fn(u8) -> bool> = match (character, dialect) {
            (Some(b'b'), _) => Some(|digit| matches!(digit, b'0' | b'1')),
            (Some(b'x'), _) => Some(|digit: u8| digit.is_ascii_hexdigit()),
            (Some(b'o'), Dialect::JsYaml4) => Some(|digit| (b'0'..=b'7').contains(&digit)),
            _ => None,
        };
        if let Some(is_radix_digit) = radix_digits {
            return resolve_digits_after_prefix(text, index + 1, character, is_radix_digit);
        }
        if dialect == Dialect::JsYaml3 {
            return resolve_digits_after_prefix(text, index, character, |digit| {
                (b'0'..=b'7').contains(&digit)
            });
        }
    }
    if character == Some(b'_') {
        return false;
    }
    let mut has_digits = false;
    while index < length {
        character = text.get(index).copied();
        if character == Some(b'_') {
            index += 1;
            continue;
        }
        if dialect == Dialect::JsYaml3 && character == Some(b':') {
            break;
        }
        if !character.is_some_and(|digit| digit.is_ascii_digit()) {
            return false;
        }
        has_digits = true;
        index += 1;
    }
    if !has_digits || character == Some(b'_') {
        return false;
    }
    if character != Some(b':') {
        return true;
    }
    std::str::from_utf8(text.get(index..).unwrap_or_default())
        .is_ok_and(|tail| matches(&SEXAGESIMAL_TAIL_PATTERN, tail))
}

fn resolve_digits_after_prefix(
    text: &[u8],
    start: usize,
    prefix_character: Option<u8>,
    is_radix_digit: fn(u8) -> bool,
) -> bool {
    let mut last_character = prefix_character;
    let mut has_digits = false;
    for &digit in text.get(start..).unwrap_or_default() {
        last_character = Some(digit);
        if digit == b'_' {
            continue;
        }
        if !is_radix_digit(digit) {
            return false;
        }
        has_digits = true;
    }
    has_digits && last_character != Some(b'_')
}

fn parse_integer_digits(digits: &str, radix: u32) -> f64 {
    if radix == 10 {
        return digits.parse::<f64>().unwrap_or(f64::NAN);
    }
    let bits_per_digit = radix.trailing_zeros();
    let mut bits: Vec<bool> = Vec::new();
    for digit in digits.chars() {
        let Some(value) = digit.to_digit(radix) else {
            break;
        };
        for bit_index in (0..bits_per_digit).rev() {
            let bit = (value >> bit_index) & 1 == 1;
            if bit || !bits.is_empty() {
                bits.push(bit);
            }
        }
    }
    if digits.is_empty()
        || !digits
            .chars()
            .next()
            .is_some_and(|first| first.is_digit(radix))
    {
        return f64::NAN;
    }
    let significant_bits = bits.len().min(64);
    let mut leading: u64 = 0;
    for &bit in bits.iter().take(significant_bits) {
        leading = (leading << 1) | u64::from(bit);
    }
    if bits.iter().skip(significant_bits).any(|bit| *bit) {
        leading |= 1;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "u64 to f64 rounds to nearest even like JavaScript parseInt; the sticky bit keeps the rounding exact"
    )]
    let mantissa = leading as f64;
    let remaining_bits = i32::try_from(bits.len() - significant_bits).unwrap_or(i32::MAX);
    mantissa * 2f64.powi(remaining_bits)
}

fn construct_integer(text: &str, dialect: Dialect) -> f64 {
    let mut value: String = text.chars().filter(|character| *character != '_').collect();
    let mut sign = 1.0;
    if let Some(stripped) = value.strip_prefix('-') {
        sign = -1.0;
        value = stripped.to_string();
    } else if let Some(stripped) = value.strip_prefix('+') {
        value = stripped.to_string();
    }
    if value == "0" {
        return 0.0;
    }
    if value.starts_with('0') {
        let digits_after_prefix = value.get(2..).unwrap_or_default();
        match (value.as_bytes().get(1), dialect) {
            (Some(b'b'), _) => return sign * parse_integer_digits(digits_after_prefix, 2),
            (Some(b'x'), _) => return sign * parse_integer_digits(digits_after_prefix, 16),
            (Some(b'o'), Dialect::JsYaml4) => {
                return sign * parse_integer_digits(digits_after_prefix, 8);
            }
            (_, Dialect::JsYaml3) => return sign * parse_integer_digits(&value, 8),
            _ => {}
        }
    }
    if dialect == Dialect::JsYaml3 && value.contains(':') {
        return sign * sexagesimal_sum(&value, |part| parse_integer_digits(part, 10));
    }
    sign * parse_integer_digits(&value, 10)
}

fn sexagesimal_sum(value: &str, parse_part: impl Fn(&str) -> f64) -> f64 {
    let mut total = 0.0;
    let mut base = 1.0;
    for part in value.split(':').rev() {
        total += parse_part(part) * base;
        base *= 60.0;
    }
    total
}

fn js_parse_float(text: &str) -> f64 {
    let trimmed = text.trim_start();
    let bytes = trimmed.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    if trimmed
        .get(end..)
        .is_some_and(|rest| rest.starts_with("Infinity"))
    {
        return if trimmed.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let count_digits = |from: usize| {
        bytes
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let integer_digits = count_digits(end);
    end += integer_digits;
    let mut fraction_digits = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction_digits = count_digits(end + 1);
        if integer_digits + fraction_digits > 0 {
            end += 1 + fraction_digits;
        }
    }
    if integer_digits + fraction_digits == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent_end = end + 1;
        if matches!(bytes.get(exponent_end), Some(b'+' | b'-')) {
            exponent_end += 1;
        }
        let exponent_digits = count_digits(exponent_end);
        if exponent_digits > 0 {
            end = exponent_end + exponent_digits;
        }
    }
    trimmed
        .get(..end)
        .and_then(|prefix| prefix.parse::<f64>().ok())
        .unwrap_or(f64::NAN)
}

fn construct_float(text: &str, dialect: Dialect) -> f64 {
    let mut value: String = text
        .chars()
        .filter(|character| *character != '_')
        .collect::<String>()
        .to_lowercase();
    let sign = if value.starts_with('-') { -1.0 } else { 1.0 };
    if value.starts_with('+') || value.starts_with('-') {
        value.remove(0);
    }
    if value == ".inf" {
        return if sign > 0.0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
    }
    if value == ".nan" {
        return f64::NAN;
    }
    if dialect == Dialect::JsYaml3 && value.contains(':') {
        return sign * sexagesimal_sum(&value, js_parse_float);
    }
    sign * js_parse_float(&value)
}

fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > MAXIMUM_TIME_VALUE {
        return f64::NAN;
    }
    time + 0.0
}

#[expect(
    clippy::cast_precision_loss,
    reason = "JavaScript Date arithmetic is done in Numbers; every intermediate stays far below 2^53"
)]
fn date_utc(year: i64, month: i64, day: i64, time_of_day_milliseconds: i64) -> f64 {
    let full_year = if (0..=99).contains(&year) {
        1900 + year
    } else {
        year
    };
    let year_with_months = full_year + month.div_euclid(12);
    let month_in_year = month.rem_euclid(12);
    let days = days_from_civil(year_with_months, month_in_year + 1, 1) + day - 1;
    time_clip((days * 86_400_000 + time_of_day_milliseconds) as f64)
}

fn capture_number(captures: &regex::Captures<'_>, index: usize) -> Option<i64> {
    captures
        .get(index)
        .and_then(|found| found.as_str().parse::<i64>().ok())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "time zone offsets in milliseconds are small integers"
)]
fn construct_timestamp(text: &str) -> Option<f64> {
    let date_pattern = DATE_PATTERN.as_ref()?;
    let timestamp_pattern = TIMESTAMP_PATTERN.as_ref()?;
    let captures = date_pattern
        .captures(text)
        .or_else(|| timestamp_pattern.captures(text))?;
    let year = capture_number(&captures, 1)?;
    let month = capture_number(&captures, 2)? - 1;
    let day = capture_number(&captures, 3)?;
    let Some(hour) = capture_number(&captures, 4) else {
        return Some(date_utc(year, month, day, 0));
    };
    let minute = capture_number(&captures, 5)?;
    let second = capture_number(&captures, 6)?;
    let fraction = captures.get(7).map_or(0, |found| {
        let mut milliseconds: String = found.as_str().chars().take(3).collect();
        while milliseconds.len() < 3 {
            milliseconds.push('0');
        }
        milliseconds.parse::<i64>().unwrap_or(0)
    });
    let mut time = date_utc(
        year,
        month,
        day,
        hour * 3_600_000 + minute * 60_000 + second * 1000 + fraction,
    );
    if let Some(sign) = captures.get(9) {
        let offset_hours = capture_number(&captures, 10).unwrap_or(0);
        let offset_minutes = capture_number(&captures, 11).unwrap_or(0);
        let mut offset = (offset_hours * 60 + offset_minutes) * 60_000;
        if sign.as_str() == "-" {
            offset = -offset;
        }
        if offset != 0 {
            time = time_clip(time - offset as f64);
        }
    }
    Some(time)
}

fn resolve_binary(text: &str) -> bool {
    let mut bit_length = 0usize;
    for unit in text.encode_utf16() {
        let code = u8::try_from(unit).ok().and_then(|byte| {
            BASE64_ALPHABET
                .iter()
                .position(|candidate| *candidate == byte)
        });
        match code {
            Some(position) if position > BASE64_PADDING_CODE => {}
            Some(_) => bit_length += 6,
            None => return false,
        }
    }
    bit_length.is_multiple_of(8)
}

fn construct_binary(text: &str) -> Vec<u8> {
    let symbols: Vec<u32> = text
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n' | b'='))
        .map(|byte| {
            BASE64_ALPHABET
                .iter()
                .position(|candidate| *candidate == byte)
                .and_then(|position| u32::try_from(position).ok())
                .unwrap_or(u32::MAX)
        })
        .collect();
    let mut bytes = Vec::new();
    let mut bits: u32 = 0;
    let low_byte = |value: u32| value.to_le_bytes()[0];
    for (index, symbol) in symbols.iter().enumerate() {
        if index % 4 == 0 && index > 0 {
            bytes.push(low_byte(bits >> 16));
            bytes.push(low_byte(bits >> 8));
            bytes.push(low_byte(bits));
        }
        bits = (bits << 6) | symbol;
    }
    match (symbols.len() % 4) * 6 {
        0 => {
            bytes.push(low_byte(bits >> 16));
            bytes.push(low_byte(bits >> 8));
            bytes.push(low_byte(bits));
        }
        18 => {
            bytes.push(low_byte(bits >> 10));
            bytes.push(low_byte(bits >> 2));
        }
        12 => bytes.push(low_byte(bits >> 4)),
        _ => {}
    }
    bytes
}
