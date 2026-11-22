use super::{Dialect, YamlError};
use crate::js;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;

// ponytail: alias bombs are cut off at a fixed value count, where Node would run out of memory or string length
const MAXIMUM_CONVERTED_VALUES: usize = 1_000_000;
const MILLISECONDS_PER_DAY: i64 = 86_400_000;
const WEEKDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Clone, Debug)]
pub(super) enum JsValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Date(f64),
    Binary(Vec<u8>),
    Array(usize),
    Object(usize),
}

impl JsValue {
    pub(super) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub(super) fn is_js_object(&self) -> bool {
        matches!(
            self,
            Self::Date(_) | Self::Binary(_) | Self::Array(_) | Self::Object(_)
        )
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct JsObject {
    entries: Vec<(String, JsValue)>,
    positions: HashMap<String, usize>,
}

impl JsObject {
    pub(super) fn has_own(&self, key: &str) -> bool {
        self.positions.contains_key(key)
    }

    pub(super) fn set(&mut self, key: String, value: JsValue) {
        if let Some(&position) = self.positions.get(&key) {
            if let Some(entry) = self.entries.get_mut(position) {
                entry.1 = value;
            }
            return;
        }
        self.positions.insert(key.clone(), self.entries.len());
        self.entries.push((key, value));
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn entries_in_enumeration_order(&self) -> Vec<(String, JsValue)> {
        let mut index_entries: Vec<(u32, &(String, JsValue))> = self
            .entries
            .iter()
            .filter_map(|entry| array_index(&entry.0).map(|index| (index, entry)))
            .collect();
        index_entries.sort_by_key(|(index, _)| *index);
        index_entries
            .into_iter()
            .map(|(_, entry)| entry.clone())
            .chain(
                self.entries
                    .iter()
                    .filter(|entry| array_index(&entry.0).is_none())
                    .cloned(),
            )
            .collect()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &JsValue> {
        self.entries.iter().map(|(_, value)| value)
    }
}

fn array_index(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    let canonical = key == "0"
        || (bytes
            .first()
            .is_some_and(|first| (b'1'..=b'9').contains(first))
            && bytes.iter().all(u8::is_ascii_digit));
    if !canonical || bytes.len() > 10 {
        return None;
    }
    key.parse::<u64>()
        .ok()
        .filter(|index| *index < 4_294_967_295)
        .and_then(|index| u32::try_from(index).ok())
}

#[derive(Debug, Default)]
pub(super) struct Heap {
    pub(super) arrays: Vec<Vec<JsValue>>,
    pub(super) objects: Vec<JsObject>,
}

impl Heap {
    pub(super) fn new_array(&mut self, items: Vec<JsValue>) -> usize {
        self.arrays.push(items);
        self.arrays.len() - 1
    }

    pub(super) fn new_object(&mut self) -> usize {
        self.objects.push(JsObject::default());
        self.objects.len() - 1
    }

    pub(super) fn own_enumerable_entries(&self, value: &JsValue) -> Vec<(String, JsValue)> {
        match value {
            JsValue::Object(id) => self.objects[*id].entries_in_enumeration_order(),
            JsValue::Array(id) => self.arrays[*id]
                .iter()
                .enumerate()
                .map(|(index, item)| (index.to_string(), item.clone()))
                .collect(),
            JsValue::Binary(bytes) => bytes
                .iter()
                .enumerate()
                .map(|(index, byte)| (index.to_string(), JsValue::Number(f64::from(*byte))))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn to_js_string(&self, value: &JsValue, dialect: Dialect) -> String {
        let mut arrays_being_joined = Vec::new();
        self.to_js_string_guarded(value, dialect, &mut arrays_being_joined)
    }

    fn to_js_string_guarded(
        &self,
        value: &JsValue,
        dialect: Dialect,
        arrays_being_joined: &mut Vec<usize>,
    ) -> String {
        match value {
            JsValue::Null => "null".to_string(),
            JsValue::Bool(flag) => flag.to_string(),
            JsValue::Number(number) => js::float_to_string(*number),
            JsValue::String(text) => text.clone(),
            JsValue::Date(time) => date_to_string(*time),
            JsValue::Binary(bytes) => match dialect {
                Dialect::JsYaml3 => String::from_utf8_lossy(bytes).into_owned(),
                Dialect::JsYaml4 => bytes
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            },
            JsValue::Array(id) => {
                if arrays_being_joined.contains(id) {
                    return String::new();
                }
                arrays_being_joined.push(*id);
                let joined = self.arrays[*id]
                    .iter()
                    .map(|item| match item {
                        JsValue::Null => String::new(),
                        other => self.to_js_string_guarded(other, dialect, arrays_being_joined),
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                arrays_being_joined.pop();
                joined
            }
            JsValue::Object(_) => "[object Object]".to_string(),
        }
    }
}

pub(super) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

pub(super) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let adjusted_year = if month <= 2 { year - 1 } else { year };
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year.rem_euclid(400);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

struct DateParts {
    year: i64,
    month: i64,
    day: i64,
    weekday: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    milliseconds: i64,
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a valid JavaScript time value is an integral number of milliseconds within 8.64e15"
)]
fn date_parts(time: f64) -> Option<DateParts> {
    if !time.is_finite() {
        return None;
    }
    let milliseconds_total = time as i64;
    let days = milliseconds_total.div_euclid(MILLISECONDS_PER_DAY);
    let time_of_day = milliseconds_total.rem_euclid(MILLISECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    Some(DateParts {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7),
        hours: time_of_day / 3_600_000,
        minutes: time_of_day / 60_000 % 60,
        seconds: time_of_day / 1000 % 60,
        milliseconds: time_of_day % 1000,
    })
}

fn iso_string(time: f64) -> Option<String> {
    let parts = date_parts(time)?;
    let year = if (0..=9999).contains(&parts.year) {
        format!("{:04}", parts.year)
    } else if parts.year < 0 {
        format!("-{:06}", -parts.year)
    } else {
        format!("+{:06}", parts.year)
    };
    Some(format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        parts.month, parts.day, parts.hours, parts.minutes, parts.seconds, parts.milliseconds
    ))
}

fn name_at(names: &[&'static str], index: i64) -> &'static str {
    usize::try_from(index)
        .ok()
        .and_then(|position| names.get(position))
        .copied()
        .unwrap_or_default()
}

// ponytail: Date#toString is rendered in UTC; Node renders the process time zone (the corpus runs with TZ=UTC)
fn date_to_string(time: f64) -> String {
    let Some(parts) = date_parts(time) else {
        return "Invalid Date".to_string();
    };
    let year = if parts.year < 0 {
        format!("-{:04}", -parts.year)
    } else {
        format!("{:04}", parts.year)
    };
    format!(
        "{} {} {:02} {year} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        name_at(&WEEKDAY_NAMES, parts.weekday),
        name_at(&MONTH_NAMES, parts.month - 1),
        parts.day,
        parts.hours,
        parts.minutes,
        parts.seconds
    )
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "integral numbers below 9e15 are exact JavaScript integers"
)]
fn number_to_json(number: f64) -> Value {
    if !number.is_finite() {
        return Value::Null;
    }
    if number.fract() == 0.0 && number.abs() < 9.0e15 {
        return Value::Number((number as i64).into());
    }
    Number::from_f64(number).map_or(Value::Null, Value::Number)
}

struct JsonConversion<'heap> {
    heap: &'heap Heap,
    dialect: Dialect,
    converted_values: usize,
    arrays_in_progress: Vec<usize>,
    objects_in_progress: Vec<usize>,
}

pub(super) fn to_json(value: &JsValue, heap: &Heap, dialect: Dialect) -> Result<Value, YamlError> {
    let mut conversion = JsonConversion {
        heap,
        dialect,
        converted_values: 0,
        arrays_in_progress: Vec::new(),
        objects_in_progress: Vec::new(),
    };
    conversion.convert(value)
}

impl JsonConversion<'_> {
    fn convert(&mut self, value: &JsValue) -> Result<Value, YamlError> {
        self.converted_values += 1;
        if self.converted_values > MAXIMUM_CONVERTED_VALUES {
            return Err(YamlError {
                name: "RangeError",
                message: "Invalid string length".to_string(),
            });
        }
        Ok(match value {
            JsValue::Null => Value::Null,
            JsValue::Bool(flag) => Value::Bool(*flag),
            JsValue::Number(number) => number_to_json(*number),
            JsValue::String(text) => Value::String(text.clone()),
            JsValue::Date(time) => iso_string(*time).map_or(Value::Null, Value::String),
            JsValue::Binary(bytes) => match self.dialect {
                Dialect::JsYaml3 => {
                    let mut buffer = Map::new();
                    buffer.insert("type".to_string(), Value::String("Buffer".to_string()));
                    buffer.insert(
                        "data".to_string(),
                        Value::Array(bytes.iter().map(|byte| Value::from(*byte)).collect()),
                    );
                    Value::Object(buffer)
                }
                Dialect::JsYaml4 => Value::Object(
                    bytes
                        .iter()
                        .enumerate()
                        .map(|(index, byte)| (index.to_string(), Value::from(*byte)))
                        .collect(),
                ),
            },
            JsValue::Array(id) => {
                if self.arrays_in_progress.contains(id) {
                    return Err(circular_structure_error());
                }
                self.arrays_in_progress.push(*id);
                let items = self.heap.arrays[*id]
                    .iter()
                    .map(|item| self.convert(item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.arrays_in_progress.pop();
                Value::Array(items)
            }
            JsValue::Object(id) => {
                if self.objects_in_progress.contains(id) {
                    return Err(circular_structure_error());
                }
                self.objects_in_progress.push(*id);
                let mut object = Map::new();
                for (key, item) in self.heap.objects[*id].entries_in_enumeration_order() {
                    let converted = self.convert(&item)?;
                    object.insert(key, converted);
                }
                self.objects_in_progress.pop();
                Value::Object(object)
            }
        })
    }
}

fn circular_structure_error() -> YamlError {
    YamlError {
        name: "TypeError",
        message: "Converting circular structure to JSON".to_string(),
    }
}
