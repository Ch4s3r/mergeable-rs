use std::fmt::Write;

const LINE_FEED: u16 = 0x0A;
const CARRIAGE_RETURN: u16 = 0x0D;
const TAB: u16 = 0x09;
const RIGHTWARDS_ARROW: u16 = 0x2192;

#[derive(Debug, Clone)]
pub(super) struct Mark {
    pub(super) buffer: Vec<u16>,
    pub(super) position: usize,
    pub(super) line: usize,
    pub(super) column: i64,
}

fn to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn spaces(count: i64) -> String {
    " ".repeat(usize::try_from(count).unwrap_or(0))
}

pub(super) fn format_js_yaml_3(reason: &str, mark: &Mark) -> String {
    let mut location = format!(
        "at line {}, column {}",
        to_i64(mark.line) + 1,
        mark.column + 1
    );
    if let Some(snippet) = js_yaml_3_snippet(mark) {
        location.push_str(":\n");
        location.push_str(&snippet);
    }
    format!("{reason} {location}")
}

fn is_snippet_boundary(unit: u16) -> bool {
    matches!(unit, 0x00 | 0x0D | 0x0A | 0x85 | 0x2028 | 0x2029)
}

fn js_yaml_3_snippet(mark: &Mark) -> Option<String> {
    const INDENT: i64 = 4;
    const HALF_WIDTH_LIMIT: usize = 36;
    let buffer = &mark.buffer;
    if buffer.is_empty() {
        return None;
    }
    let position = mark.position.min(buffer.len());
    let mut head = "";
    let mut start = position;
    while start > 0
        && !buffer
            .get(start - 1)
            .copied()
            .is_some_and(is_snippet_boundary)
    {
        start -= 1;
        if position - start > HALF_WIDTH_LIMIT {
            head = " ... ";
            start += 5;
            break;
        }
    }
    let mut tail = "";
    let mut end = position;
    while end < buffer.len() && !buffer.get(end).copied().is_some_and(is_snippet_boundary) {
        end += 1;
        if end - position > HALF_WIDTH_LIMIT {
            tail = " ... ";
            end -= 5;
            break;
        }
    }
    let snippet = String::from_utf16_lossy(buffer.get(start..end).unwrap_or_default());
    Some(format!(
        "{}{head}{snippet}{tail}\n{}^",
        spaces(INDENT),
        spaces(INDENT + to_i64(position - start) + to_i64(head.len()))
    ))
}

pub(super) fn format_js_yaml_4(reason: &str, mark: &Mark) -> String {
    let mut location = format!("({}:{})", to_i64(mark.line) + 1, mark.column + 1);
    if let Some(snippet) = js_yaml_4_snippet(mark) {
        location.push_str("\n\n");
        location.push_str(&snippet);
    }
    format!("{reason} {location}")
}

fn js_slice(buffer: &[u16], start: i64, end: Option<i64>) -> &[u16] {
    let length = to_i64(buffer.len());
    let clamp = |index: i64| {
        let absolute = if index < 0 { length + index } else { index };
        usize::try_from(absolute.clamp(0, length)).unwrap_or(0)
    };
    let from = clamp(start);
    let to = end.map_or(buffer.len(), clamp);
    if from >= to {
        return &[];
    }
    buffer.get(from..to).unwrap_or_default()
}

struct SnippetLine {
    text: String,
    caret_offset: i64,
}

fn snippet_line(
    buffer: &[u16],
    mut line_start: i64,
    mut line_end: Option<i64>,
    position: i64,
    maximum_line_length: i64,
) -> SnippetLine {
    let maximum_half_length = maximum_line_length.div_euclid(2) - 1;
    let mut head = "";
    let mut tail = "";
    if position - line_start > maximum_half_length {
        head = " ... ";
        line_start = position - maximum_half_length + to_i64(head.len());
    }
    if let Some(end) = line_end
        && end - position > maximum_half_length
    {
        tail = " ...";
        line_end = Some(position + maximum_half_length - to_i64(tail.len()));
    }
    let units: Vec<u16> = js_slice(buffer, line_start, line_end)
        .iter()
        .map(|unit| {
            if *unit == TAB {
                RIGHTWARDS_ARROW
            } else {
                *unit
            }
        })
        .collect();
    SnippetLine {
        text: format!("{head}{}{tail}", String::from_utf16_lossy(&units)),
        caret_offset: position - line_start + to_i64(head.len()),
    }
}

fn pad_start(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(text.len())))
}

struct LineTable {
    starts: Vec<i64>,
    ends: Vec<i64>,
    found_line: usize,
}

fn line_table(buffer: &[u16], position: i64) -> LineTable {
    let mut starts: Vec<i64> = vec![0];
    let mut ends: Vec<i64> = Vec::new();
    let mut found_line: Option<usize> = None;
    let mut index = 0;
    while index < buffer.len() {
        let unit = buffer.get(index).copied().unwrap_or_default();
        let break_length = if unit == CARRIAGE_RETURN && buffer.get(index + 1) == Some(&LINE_FEED) {
            2
        } else {
            usize::from(matches!(unit, LINE_FEED | CARRIAGE_RETURN | 0))
        };
        if break_length == 0 {
            index += 1;
            continue;
        }
        ends.push(to_i64(index));
        starts.push(to_i64(index + break_length));
        if position <= to_i64(index) && found_line.is_none() {
            found_line = Some(starts.len() - 2);
        }
        index += break_length;
    }
    let found_line = found_line.unwrap_or(starts.len() - 1);
    LineTable {
        starts,
        ends,
        found_line,
    }
}

fn js_yaml_4_snippet(mark: &Mark) -> Option<String> {
    const MAXIMUM_LENGTH: i64 = 79;
    const INDENT: i64 = 1;
    const LINES_BEFORE: i64 = 3;
    const LINES_AFTER: i64 = 2;
    let buffer = &mark.buffer;
    if buffer.is_empty() {
        return None;
    }
    let position = to_i64(mark.position);
    let mark_line = to_i64(mark.line);
    let LineTable {
        starts: line_starts,
        ends: line_ends,
        found_line,
    } = line_table(buffer, position);
    let start_of = |line: usize| line_starts.get(line).copied().unwrap_or_default();
    let end_of = |line: usize| line_ends.get(line).copied();
    let line_number_width = (mark_line + LINES_AFTER)
        .min(to_i64(line_ends.len()))
        .to_string()
        .len();
    let maximum_line_length = MAXIMUM_LENGTH - (INDENT + to_i64(line_number_width) + 3);
    let indent = spaces(INDENT);
    let mut before = String::new();
    for offset in 1..=LINES_BEFORE {
        let Some(line) = usize::try_from(offset)
            .ok()
            .and_then(|offset| found_line.checked_sub(offset))
        else {
            break;
        };
        let rendered = snippet_line(
            buffer,
            start_of(line),
            end_of(line),
            position - (start_of(found_line) - start_of(line)),
            maximum_line_length,
        );
        before = format!(
            "{indent}{} | {}\n{before}",
            pad_start(&(mark_line - offset + 1).to_string(), line_number_width),
            rendered.text
        );
    }
    let current = snippet_line(
        buffer,
        start_of(found_line),
        end_of(found_line),
        position,
        maximum_line_length,
    );
    let mut result = before;
    let _ = writeln!(
        result,
        "{indent}{} | {}",
        pad_start(&(mark_line + 1).to_string(), line_number_width),
        current.text
    );
    result.push_str(&"-".repeat(
        usize::try_from(INDENT + to_i64(line_number_width) + 3 + current.caret_offset).unwrap_or(0),
    ));
    result.push_str("^\n");
    for offset in 1..=LINES_AFTER {
        let line = found_line + usize::try_from(offset).unwrap_or(0);
        if line >= line_ends.len() {
            break;
        }
        let rendered = snippet_line(
            buffer,
            start_of(line),
            end_of(line),
            position - (start_of(found_line) - start_of(line)),
            maximum_line_length,
        );
        let _ = writeln!(
            result,
            "{indent}{} | {}",
            pad_start(&(mark_line + offset + 1).to_string(), line_number_width),
            rendered.text
        );
    }
    if result.ends_with('\n') {
        result.pop();
    }
    Some(result)
}
