//! Ports of the Go standard library behaviors whose text reaches the user:
//! strconv number and bool parsing, strconv quoting, and encoding/csv
//! single-record reading as pflag's stringSlice uses it.

use super::isprint::{IS_NOT_PRINT16, IS_NOT_PRINT32, IS_PRINT16, IS_PRINT32};

const SYNTAX: &str = "invalid syntax";
const RANGE: &str = "value out of range";

fn num_error(func: &str, input: &str, err: &str) -> String {
    format!("strconv.{func}: parsing {}: {err}", quote(input))
}

pub fn parse_bool(s: &str) -> Result<bool, String> {
    match s {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Ok(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Ok(false),
        _ => Err(num_error("ParseBool", s, SYNTAX)),
    }
}

enum UintError {
    Syntax,
    Range,
}

/// strconv.ParseUint(s, 0, 64) without the error wrapping.
fn parse_uint_base0(s: &str) -> Result<u64, UintError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(UintError::Syntax);
    }
    let (base, digits): (u64, &[u8]) = match bytes {
        [b'0', p, rest @ ..] if !rest.is_empty() && p.eq_ignore_ascii_case(&b'b') => (2, rest),
        [b'0', p, rest @ ..] if !rest.is_empty() && p.eq_ignore_ascii_case(&b'o') => (8, rest),
        [b'0', p, rest @ ..] if !rest.is_empty() && p.eq_ignore_ascii_case(&b'x') => (16, rest),
        [b'0', rest @ ..] => (8, rest),
        _ => (10, bytes),
    };
    let cutoff = u64::MAX / base + 1;
    let mut n: u64 = 0;
    let mut underscores = false;
    for &c in digits {
        let d = match c.to_ascii_lowercase() {
            b'_' => {
                underscores = true;
                continue;
            }
            c @ b'0'..=b'9' => c - b'0',
            c @ b'a'..=b'z' => c - b'a' + 10,
            _ => return Err(UintError::Syntax),
        };
        if u64::from(d) >= base {
            return Err(UintError::Syntax);
        }
        if n >= cutoff {
            return Err(UintError::Range);
        }
        n = match (n * base).checked_add(u64::from(d)) {
            Some(n) => n,
            None => return Err(UintError::Range),
        };
    }
    if underscores && !underscore_ok(s) {
        return Err(UintError::Syntax);
    }
    Ok(n)
}

/// Go's rule that base-prefixed literals may separate digits with single
/// underscores.
fn underscore_ok(s: &str) -> bool {
    let mut s = s.as_bytes();
    if let [b'-' | b'+', rest @ ..] = s {
        s = rest;
    }
    let mut saw = b'^';
    let mut i = 0;
    let mut hex = false;
    if s.len() >= 2 && s[0] == b'0' && matches!(s[1].to_ascii_lowercase(), b'b' | b'o' | b'x') {
        i = 2;
        saw = b'0';
        hex = s[1].eq_ignore_ascii_case(&b'x');
    }
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_digit() || hex && matches!(c.to_ascii_lowercase(), b'a'..=b'f') {
            saw = b'0';
        } else if c == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else {
            if saw == b'_' {
                return false;
            }
            saw = b'!';
        }
        i += 1;
    }
    saw != b'_'
}

pub fn parse_uint(s: &str) -> Result<u64, String> {
    parse_uint_base0(s).map_err(|e| match e {
        UintError::Syntax => num_error("ParseUint", s, SYNTAX),
        UintError::Range => num_error("ParseUint", s, RANGE),
    })
}

pub fn parse_int(s: &str) -> Result<i64, String> {
    if s.is_empty() {
        return Err(num_error("ParseInt", s, SYNTAX));
    }
    let (neg, body) = match s.as_bytes()[0] {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    let un = match parse_uint_base0(body) {
        Ok(n) => n,
        Err(UintError::Range) => u64::MAX,
        Err(UintError::Syntax) => return Err(num_error("ParseInt", s, SYNTAX)),
    };
    const CUTOFF: u64 = 1 << 63;
    if !neg && un >= CUTOFF || neg && un > CUTOFF {
        return Err(num_error("ParseInt", s, RANGE));
    }
    Ok(if neg {
        (un as i64).wrapping_neg()
    } else {
        un as i64
    })
}

fn is_print(r: char) -> bool {
    let r = r as u32;
    if r <= 0xFF {
        return (0x20..=0x7E).contains(&r) || (0xA1..=0xFF).contains(&r) && r != 0xAD;
    }
    if r < 1 << 16 {
        let rr = r as u16;
        let i = IS_PRINT16.partition_point(|&x| x < rr);
        if i >= IS_PRINT16.len() || rr < IS_PRINT16[i & !1] || IS_PRINT16[i | 1] < rr {
            return false;
        }
        return IS_NOT_PRINT16.binary_search(&rr).is_err();
    }
    let i = IS_PRINT32.partition_point(|&x| x < r);
    if i >= IS_PRINT32.len() || r < IS_PRINT32[i & !1] || IS_PRINT32[i | 1] < r {
        return false;
    }
    if r >= 0x20000 {
        return true;
    }
    IS_NOT_PRINT32
        .binary_search(&((r - 0x10000) as u16))
        .is_err()
}

fn push_escaped(buf: &mut String, r: char, quote: char) {
    use std::fmt::Write;
    if r == quote || r == '\\' {
        buf.push('\\');
        buf.push(r);
    } else if is_print(r) {
        buf.push(r);
    } else {
        match r {
            '\x07' => buf.push_str("\\a"),
            '\x08' => buf.push_str("\\b"),
            '\x0C' => buf.push_str("\\f"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            '\x0B' => buf.push_str("\\v"),
            r if r < ' ' || r == '\x7F' => write!(buf, "\\x{:02x}", r as u32).unwrap(),
            r if (r as u32) < 0x10000 => write!(buf, "\\u{:04x}", r as u32).unwrap(),
            r => write!(buf, "\\U{:08x}", r as u32).unwrap(),
        }
    }
}

/// Go's strconv.Quote, which `%q` uses for strings.
pub fn quote(s: &str) -> String {
    let mut buf = String::with_capacity(s.len() + 2);
    buf.push('"');
    for r in s.chars() {
        push_escaped(&mut buf, r, '"');
    }
    buf.push('"');
    buf
}

/// Go's strconv.QuoteRune, which `%q` uses for runes.
pub fn quote_rune(r: char) -> String {
    let mut buf = String::from('\'');
    push_escaped(&mut buf, r, '\'');
    buf.push('\'');
    buf
}

/// Go's `%#q`: a raw backquoted string when strconv.CanBackquote allows it.
pub fn backquote(s: &str) -> String {
    let can = s
        .chars()
        .all(|r| r != '\u{FEFF}' && r != '`' && r != '\x7F' && (r >= ' ' || r == '\t'));
    if can { format!("`{s}`") } else { quote(s) }
}

/// One record of encoding/csv with the default Reader settings, as
/// pflag's readAsCSV calls it on a non-empty value.
pub fn read_csv_record(input: &str) -> Result<Vec<String>, String> {
    let mut reader = CsvLines {
        rest: input.as_bytes(),
        line: 0,
    };

    let mut line = loop {
        match reader.read_line() {
            None => return Err("EOF".to_string()),
            Some(l) if l.len() == newline_len(&l) => continue,
            Some(l) => break l,
        }
    };

    let rec_line = reader.line;
    let mut record: Vec<u8> = Vec::new();
    let mut ends: Vec<usize> = Vec::new();
    let (mut pos_line, mut pos_col) = (reader.line, 1usize);
    let parse_error = |line: usize, col: usize, err: &str| {
        if rec_line != line {
            format!("record on line {rec_line}; parse error on line {line}, column {col}: {err}")
        } else {
            format!("parse error on line {line}, column {col}: {err}")
        }
    };
    const BARE: &str = "bare \" in non-quoted-field";
    const QUOTE: &str = "extraneous or missing \" in quoted-field";

    'field: loop {
        if line.first() != Some(&b'"') {
            let comma = line.iter().position(|&b| b == b',');
            let field = match comma {
                Some(i) => &line[..i],
                None => &line[..line.len() - newline_len(&line)],
            };
            if let Some(j) = field.iter().position(|&b| b == b'"') {
                return Err(parse_error(reader.line, pos_col + j, BARE));
            }
            record.extend_from_slice(field);
            ends.push(record.len());
            match comma {
                Some(i) => {
                    line = line[i + 1..].to_vec();
                    pos_col += i + 1;
                }
                None => break 'field,
            }
        } else {
            line = line[1..].to_vec();
            pos_col += 1;
            loop {
                if let Some(i) = line.iter().position(|&b| b == b'"') {
                    record.extend_from_slice(&line[..i]);
                    line = line[i + 1..].to_vec();
                    pos_col += i + 1;
                    match line.first() {
                        Some(b'"') => {
                            record.push(b'"');
                            line = line[1..].to_vec();
                            pos_col += 1;
                        }
                        Some(b',') => {
                            line = line[1..].to_vec();
                            pos_col += 1;
                            ends.push(record.len());
                            continue 'field;
                        }
                        _ if newline_len(&line) == line.len() => {
                            ends.push(record.len());
                            break 'field;
                        }
                        _ => return Err(parse_error(reader.line, pos_col - 1, QUOTE)),
                    }
                } else if !line.is_empty() {
                    record.extend_from_slice(&line);
                    pos_col += line.len();
                    line = reader.read_line().unwrap_or_default();
                    if !line.is_empty() {
                        pos_line += 1;
                        pos_col = 1;
                    }
                } else {
                    return Err(parse_error(pos_line, pos_col, QUOTE));
                }
            }
        }
    }

    let text = String::from_utf8_lossy(&record).into_owned();
    let mut start = 0;
    Ok(ends
        .into_iter()
        .map(|end| {
            let field = text[start..end].to_string();
            start = end;
            field
        })
        .collect())
}

fn newline_len(b: &[u8]) -> usize {
    usize::from(b.last() == Some(&b'\n'))
}

struct CsvLines<'a> {
    rest: &'a [u8],
    line: usize,
}

impl CsvLines<'_> {
    /// csv.Reader.readLine: one line with `\r\n` normalized to `\n` and a
    /// trailing `\r` before EOF dropped; None at EOF.
    fn read_line(&mut self) -> Option<Vec<u8>> {
        if self.rest.is_empty() {
            return None;
        }
        let end = self
            .rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(self.rest.len(), |i| i + 1);
        let mut line = self.rest[..end].to_vec();
        self.rest = &self.rest[end..];
        if self.rest.is_empty() && line.last() == Some(&b'\r') {
            line.pop();
        }
        self.line += 1;
        let n = line.len();
        if n >= 2 && line[n - 2] == b'\r' && line[n - 1] == b'\n' {
            line[n - 2] = b'\n';
            line.pop();
        }
        Some(line)
    }
}

/// strings.TrimRightFunc(s, unicode.IsSpace).
pub fn trim_right_space(s: &str) -> &str {
    s.trim_end_matches(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ints_like_strconv() {
        assert_eq!(parse_int("42"), Ok(42));
        assert_eq!(parse_int("-0x1_0"), Ok(-16));
        assert_eq!(parse_int("0o17"), Ok(15));
        assert_eq!(parse_int("017"), Ok(15));
        assert_eq!(parse_int("0b101"), Ok(5));
        assert_eq!(parse_int("0"), Ok(0));
        assert_eq!(parse_int("-9223372036854775808"), Ok(i64::MIN));
        assert_eq!(
            parse_int("x"),
            Err(r#"strconv.ParseInt: parsing "x": invalid syntax"#.into())
        );
        assert_eq!(
            parse_int(""),
            Err(r#"strconv.ParseInt: parsing "": invalid syntax"#.into())
        );
        assert_eq!(
            parse_int("1__0"),
            Err(r#"strconv.ParseInt: parsing "1__0": invalid syntax"#.into())
        );
        assert_eq!(
            parse_int("08"),
            Err(r#"strconv.ParseInt: parsing "08": invalid syntax"#.into())
        );
        assert_eq!(
            parse_int("9223372036854775808"),
            Err(r#"strconv.ParseInt: parsing "9223372036854775808": value out of range"#.into())
        );
        assert_eq!(
            parse_int("99999999999999999999x"),
            Err(r#"strconv.ParseInt: parsing "99999999999999999999x": value out of range"#.into())
        );
    }

    #[test]
    fn parses_uints_like_strconv() {
        assert_eq!(parse_uint("18446744073709551615"), Ok(u64::MAX));
        assert_eq!(
            parse_uint("-5"),
            Err(r#"strconv.ParseUint: parsing "-5": invalid syntax"#.into())
        );
        assert_eq!(
            parse_uint("+5"),
            Err(r#"strconv.ParseUint: parsing "+5": invalid syntax"#.into())
        );
        assert_eq!(
            parse_uint("18446744073709551616"),
            Err(r#"strconv.ParseUint: parsing "18446744073709551616": value out of range"#.into())
        );
    }

    #[test]
    fn parses_bools_like_strconv() {
        assert_eq!(parse_bool("T"), Ok(true));
        assert_eq!(parse_bool("False"), Ok(false));
        assert_eq!(
            parse_bool("maybe"),
            Err(r#"strconv.ParseBool: parsing "maybe": invalid syntax"#.into())
        );
    }

    #[test]
    fn quotes_like_strconv() {
        assert_eq!(quote("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(quote("\n\t\x01\x7f"), r#""\n\t\x01\x7f""#);
        assert_eq!(quote("youtube🐰é"), "\"youtube🐰é\"");
        assert_eq!(
            quote("\u{00AD}\u{200B}\u{E000}"),
            "\"\\u00ad\\u200b\\ue000\""
        );
        assert_eq!(quote("\u{F0000}"), r#""\U000f0000""#);
        assert_eq!(quote_rune('z'), "'z'");
        assert_eq!(quote_rune('\''), r"'\''");
        assert_eq!(backquote("nope"), "`nope`");
        assert_eq!(backquote("b`c"), r#""b`c""#);
    }

    #[test]
    fn reads_csv_records() {
        let ok = |s: &str| read_csv_record(s).unwrap();
        assert_eq!(ok("a,b"), ["a", "b"]);
        assert_eq!(ok(r#""a,b",c"#), ["a,b", "c"]);
        assert_eq!(ok(r#""say ""hi""",x"#), [r#"say "hi""#, "x"]);
        assert_eq!(ok("a,"), ["a", ""]);
        assert_eq!(ok("a\nb"), ["a"]);
        assert_eq!(ok("\n\nb,c"), ["b", "c"]);
        assert_eq!(ok("a\r\n"), ["a"]);
        assert_eq!(ok("\"a\nb\",c"), ["a\nb", "c"]);
        assert_eq!(read_csv_record("\n"), Err("EOF".into()));
        assert_eq!(
            read_csv_record("a\"b"),
            Err("parse error on line 1, column 2: bare \" in non-quoted-field".into())
        );
        assert_eq!(
            read_csv_record("\"a"),
            Err("parse error on line 1, column 3: extraneous or missing \" in quoted-field".into())
        );
        assert_eq!(
            read_csv_record("\"a\"x"),
            Err("parse error on line 1, column 3: extraneous or missing \" in quoted-field".into())
        );
        assert_eq!(
            read_csv_record("\"a\nb\"x"),
            Err("record on line 1; parse error on line 2, column 2: extraneous or missing \" in quoted-field".into())
        );
    }
}
