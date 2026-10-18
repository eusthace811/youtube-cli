//! Tables rendered like go-pretty v6 with yutu's style: StyleBoxLight with
//! no borders or separators, upper-case headers, one space of padding on
//! each side, numeric columns right-aligned, and no trailing newline.

use super::widths::{EAST_ASIAN, NARROW};
use std::sync::OnceLock;

/// One table cell. go-pretty right-aligns a column when every data cell in
/// it is a Go number.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Str(String),
    Int(i64),
    Uint(u64),
    Float(f64),
}

impl Cell {
    fn text(&self) -> String {
        match self {
            Cell::Str(s) => s.clone(),
            Cell::Int(i) => i.to_string(),
            Cell::Uint(u) => u.to_string(),
            Cell::Float(f) => go_float(*f),
        }
    }

    fn is_number(&self) -> bool {
        !matches!(self, Cell::Str(_))
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Cell {
        Cell::Str(s.to_string())
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Cell {
        Cell::Str(s)
    }
}

/// fmt's %v for float64.
fn go_float(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let s = yutu_api::yaml::format_float(f);
    if let Some((m, e)) = s.split_once("e+") {
        return format!("{m}e+{e}");
    }
    s
}

fn east_asian() -> bool {
    static EA: OnceLock<bool> = OnceLock::new();
    *EA.get_or_init(|| match std::env::var("RUNEWIDTH_EASTASIAN") {
        Ok(v) if !v.is_empty() => v == "1",
        _ => is_east_asian_locale(),
    })
}

/// go-runewidth `IsEastAsian`: the locale's charset is multi-byte and not
/// UTF-8, or the language is Japanese, Korean or Chinese.
fn is_east_asian_locale() -> bool {
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .map(|k| std::env::var(k).unwrap_or_default())
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    if locale == "POSIX"
        || locale == "C"
        || (locale.len() > 1
            && locale.starts_with('C')
            && matches!(locale.as_bytes()[1], b'.' | b'-'))
    {
        return false;
    }
    let lower_letters = locale.bytes().take_while(u8::is_ascii_lowercase).count();
    let mut charset = locale.to_lowercase();
    if (2..=3).contains(&lower_letters) {
        let mut rest = &locale[lower_letters..];
        let b = rest.as_bytes();
        if b.len() >= 3 && b[0] == b'_' && b[1].is_ascii_uppercase() && b[2].is_ascii_uppercase() {
            rest = &rest[3..];
        }
        if let Some(cs) = rest.strip_prefix('.')
            && !cs.is_empty()
        {
            charset = cs.to_lowercase();
        }
    }
    if charset.ends_with("@cjk_narrow") {
        return false;
    }
    if let Some(i) = charset.find('@') {
        charset.truncate(i);
    }
    let max = match charset.as_str() {
        "utf-8" | "utf8" => 6,
        "jis" => 8,
        "eucjp" => 3,
        "euckr" | "euccn" | "sjis" | "cp932" | "cp51932" | "cp936" | "cp949" | "cp950" | "big5"
        | "gbk" | "gb2312" => 2,
        _ => 1,
    };
    max > 1
        && (!charset.starts_with('u')
            || locale.starts_with("ja")
            || locale.starts_with("ko")
            || locale.starts_with("zh"))
}

pub fn rune_width(c: char) -> usize {
    let table = if east_asian() { EAST_ASIAN } else { NARROW };
    let v = c as u32;
    match table.binary_search_by(|&(lo, hi, _)| {
        if v < lo {
            std::cmp::Ordering::Greater
        } else if v > hi {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => table[i].2 as usize,
        Err(_) => 1,
    }
}

/// Display width ignoring ANSI escape sequences, as go-pretty measures.
pub fn width(s: &str) -> usize {
    let mut w = 0;
    let mut esc = Esc::None;
    for c in s.chars() {
        esc = match (esc, c) {
            (Esc::None, '\x1b') => Esc::Start,
            (Esc::Start, '[') => Esc::Csi,
            (Esc::Start, ']') => Esc::Osc,
            (Esc::Start, _) => Esc::None,
            (Esc::Csi, c) if c.is_ascii_alphabetic() => Esc::None,
            (Esc::Csi, _) => Esc::Csi,
            (Esc::Osc, '\x07') => Esc::None,
            (Esc::Osc, '\\') => Esc::None,
            (Esc::Osc, _) => Esc::Osc,
            (Esc::None, c) => {
                w += rune_width(c);
                Esc::None
            }
        };
    }
    w
}

#[derive(Clone, Copy)]
enum Esc {
    None,
    Start,
    Csi,
    Osc,
}

/// go-pretty `text.ProcessCRLF`: CRLF becomes LF and a lone CR moves the
/// cursor back to the start of the line, overwriting.
fn process_crlf(s: &str) -> String {
    let s = s.replace("\r\n", "\n");
    if !s.contains('\r') {
        return s;
    }
    s.split('\n')
        .map(|line| {
            if !line.contains('\r') {
                return line.to_string();
            }
            let mut out: Vec<char> = Vec::new();
            let mut pos = 0;
            for c in line.chars() {
                if c == '\r' {
                    pos = 0;
                    continue;
                }
                if pos < out.len() {
                    out[pos] = c;
                } else {
                    out.push(c);
                }
                pos += 1;
            }
            out.into_iter().collect()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stringify(text: &str) -> String {
    process_crlf(&text.replace('\t', "    "))
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Right,
}

fn align(text: &str, max: usize, a: Align) -> String {
    let text = match a {
        Align::Left => text.trim_end_matches(' '),
        Align::Right => text.trim_start_matches(' '),
    };
    let pad = max.saturating_sub(width(text));
    match a {
        Align::Left => format!("{text}{}", " ".repeat(pad)),
        Align::Right => format!("{}{text}", " ".repeat(pad)),
    }
}

pub fn render(header: &[&str], rows: &[Vec<Cell>]) -> String {
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    let numeric: Vec<bool> = (0..columns)
        .map(|i| rows.iter().all(|r| r.get(i).is_none_or(Cell::is_number)))
        .collect();
    let head: Vec<String> = (0..columns)
        .map(|i| stringify(header.get(i).copied().unwrap_or("")))
        .collect();
    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            (0..columns)
                .map(|i| r.get(i).map(|c| stringify(&c.text())).unwrap_or_default())
                .collect()
        })
        .collect();
    let mut widths = vec![0usize; columns];
    for row in std::iter::once(&head).chain(&body) {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.split('\n').map(width).max().unwrap_or(0));
        }
    }
    let mut out = String::new();
    let mut emit = |row: &[String], upper: bool| {
        let lines: Vec<Vec<&str>> = row.iter().map(|c| c.split('\n').collect()).collect();
        let height = lines.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            if !out.is_empty() {
                out.push('\n');
            }
            for (i, cell_lines) in lines.iter().enumerate() {
                let text = cell_lines.get(line).copied().unwrap_or("");
                let text = if upper {
                    text.to_uppercase()
                } else {
                    text.to_string()
                };
                let a = if numeric[i] {
                    Align::Right
                } else {
                    Align::Left
                };
                out.push(' ');
                out.push_str(&align(&text, widths[i], a));
                out.push(' ');
            }
        }
    };
    emit(&head, true);
    for row in &body {
        emit(row, false);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_like_go_pretty() {
        let rows = vec![
            vec![
                Cell::from("id-0"),
                Cell::from("Ünïcödé ✓ 日本語のタイトル"),
                Cell::Uint(620),
            ],
            vec![Cell::from("zero-0"), Cell::from(""), Cell::Uint(0)],
        ];
        assert_eq!(
            render(&["ID", "Title", "Views"], &rows),
            " ID      TITLE                       VIEWS \n id-0    Ünïcödé ✓ 日本語のタイトル    620 \n zero-0                                  0 "
        );
    }

    #[test]
    fn splits_multiline_cells_and_expands_tabs() {
        let rows = vec![vec![Cell::from("a"), Cell::from("tab\tand\nnewline")]];
        assert_eq!(
            render(&["ID", "Text"], &rows),
            " ID  TEXT       \n a   tab    and \n     newline    "
        );
    }

    #[test]
    fn empty_table_is_the_header() {
        assert_eq!(
            render(&["ID", "Title", "Channel ID", "Views"], &[]),
            " ID  TITLE  CHANNEL ID  VIEWS "
        );
    }
}
