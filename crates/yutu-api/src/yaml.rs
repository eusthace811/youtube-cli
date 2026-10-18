//! Encodes [`Value`] the way gopkg.in/yaml.v3 marshals the Go structs: keys
//! are lowercased Go field names, every exported field is written, nested
//! blocks indent by four, and scalar styles follow yaml.v3's resolver and
//! emitter rules. Only block collections are produced, plus `[]` and `{}`
//! for empty ones, which is all yutu's output needs.

use crate::schema::{Kind, type_def};
use crate::value::Value;

const BEST_INDENT: i32 = 4;

/// yaml.v3 `Marshal` of `[]*T`.
pub fn list(items: &[Value]) -> String {
    let mut e = Emitter::new();
    e.node(
        &Node::Seq(items.iter().map(node).collect()),
        Parent::Root,
        false,
    );
    e.finish()
}

/// yaml.v3 `Marshal` of one `*T`.
pub fn value(v: &Value) -> String {
    let mut e = Emitter::new();
    e.node(&node(v), Parent::Root, false);
    e.finish()
}

enum Node {
    Scalar(String, Style),
    Seq(Vec<Node>),
    Map(Vec<(String, Node)>),
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    /// A string: the style is chosen from its content.
    Str,
    /// null, bools and numbers, always plain.
    Plain,
}

fn node(v: &Value) -> Node {
    match v {
        Value::Nil => Node::Scalar("null".into(), Style::Plain),
        Value::Bool(b) => Node::Scalar(b.to_string(), Style::Plain),
        Value::Int(i) => Node::Scalar(i.to_string(), Style::Plain),
        Value::Uint(u) => Node::Scalar(u.to_string(), Style::Plain),
        Value::Float(f) => Node::Scalar(format_float(*f), Style::Plain),
        Value::Str(s) => Node::Scalar(s.clone(), Style::Str),
        Value::Strings(v) => Node::Seq(
            v.iter()
                .map(|s| Node::Scalar(s.clone(), Style::Str))
                .collect(),
        ),
        Value::List(v) => Node::Seq(v.iter().map(node).collect()),
        Value::Header(h) => {
            let mut h: Vec<_> = h.iter().collect();
            h.sort_by(|a, b| key_order(&a.0, &b.0));
            Node::Map(
                h.into_iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Node::Seq(
                                v.iter()
                                    .map(|s| Node::Scalar(s.clone(), Style::Str))
                                    .collect(),
                            ),
                        )
                    })
                    .collect(),
            )
        }
        Value::Map(m) => {
            let mut m: Vec<_> = m.iter().collect();
            m.sort_by(|a, b| key_order(&a.0, &b.0));
            Node::Map(m.into_iter().map(|(k, v)| (k.clone(), node(v))).collect())
        }
        Value::Struct(s) => Node::Map(
            type_def(s.ty)
                .fields
                .iter()
                .zip(&s.fields)
                .map(|(f, v)| (f.go.to_lowercase(), field_node(f.kind, v)))
                .collect(),
        ),
    }
}

/// A nil slice or map marshals as an empty collection; only pointers are null.
fn field_node(kind: Kind, v: &Value) -> Node {
    match (kind, v) {
        (Kind::Strings | Kind::PtrSlice(_), Value::Nil) => Node::Seq(Vec::new()),
        (Kind::Header | Kind::Map(_), Value::Nil) => Node::Map(Vec::new()),
        _ => node(v),
    }
}

/// yaml.v3 `keyList.Less` for string keys: runs of digits compare as numbers.
fn key_order(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let less = |a: &str, b: &str| -> bool {
        let ar: Vec<char> = a.chars().collect();
        let br: Vec<char> = b.chars().collect();
        let mut digits = false;
        let mut i = 0;
        while i < ar.len() && i < br.len() {
            if ar[i] == br[i] {
                digits = ar[i].is_numeric();
                i += 1;
                continue;
            }
            let al = ar[i].is_alphabetic();
            let bl = br[i].is_alphabetic();
            if al && bl {
                return ar[i] < br[i];
            }
            if al || bl {
                return if digits { al } else { bl };
            }
            let (mut an, mut bn) = (0i64, 0i64);
            if ar[i] == '0' || br[i] == '0' {
                let mut j = i;
                while j > 0 && ar[j - 1].is_numeric() {
                    j -= 1;
                    if ar[j] != '0' {
                        an = 1;
                        bn = 1;
                        break;
                    }
                }
            }
            let mut ai = i;
            while ai < ar.len() && ar[ai].is_numeric() {
                an = an.wrapping_mul(10).wrapping_add(ar[ai] as i64 - '0' as i64);
                ai += 1;
            }
            let mut bi = i;
            while bi < br.len() && br[bi].is_numeric() {
                bn = bn.wrapping_mul(10).wrapping_add(br[bi] as i64 - '0' as i64);
                bi += 1;
            }
            if an != bn {
                return an < bn;
            }
            if ai != bi {
                return ai < bi;
            }
            return ar[i] < br[i];
        }
        ar.len() < br.len()
    };
    if less(a, b) {
        Ordering::Less
    } else if less(b, a) {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

/// strconv.FormatFloat(f, 'g', -1, 64) with yaml.v3's names for infinities.
pub fn format_float(f: f64) -> String {
    if f.is_nan() {
        return ".nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { ".inf" } else { "-.inf" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let e = format!("{:e}", f.abs());
    let (mant, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let sign = if f < 0.0 { "-" } else { "" };
    if !(-4..6).contains(&exp) {
        let rest = &digits[1..];
        let m = if rest.is_empty() {
            digits[..1].to_string()
        } else {
            format!("{}.{rest}", &digits[..1])
        };
        let es = if exp < 0 { '-' } else { '+' };
        return format!("{sign}{m}e{es}{:02}", exp.abs());
    }
    let dp = exp + 1;
    let s = if dp <= 0 {
        format!("0.{}{digits}", "0".repeat((-dp) as usize))
    } else if dp as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(dp as usize - digits.len()))
    } else {
        format!("{}.{}", &digits[..dp as usize], &digits[dp as usize..])
    };
    format!("{sign}{s}")
}

#[derive(Clone, Copy, PartialEq)]
enum Parent {
    Root,
    SeqItem,
    MapValue,
    MapKey,
}

#[derive(Clone, Copy, PartialEq)]
enum ScalarStyle {
    Plain,
    Single,
    Double,
    Literal,
}

struct Emitter {
    out: String,
    column: i32,
    indent: i32,
    indents: Vec<i32>,
    whitespace: bool,
    indention: bool,
}

impl Emitter {
    fn new() -> Self {
        Emitter {
            out: String::new(),
            column: 0,
            indent: -1,
            indents: Vec::new(),
            whitespace: true,
            indention: true,
        }
    }

    fn finish(mut self) -> String {
        self.write_indent();
        self.out
    }

    fn put(&mut self, c: char) {
        self.out.push(c);
        self.column += 1;
    }

    fn put_break(&mut self) {
        self.out.push('\n');
        self.column = 0;
        self.indention = true;
    }

    fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.put(c);
        }
    }

    fn increase_indent(&mut self, flow: bool, parent: Parent) {
        self.indents.push(self.indent);
        if self.indent < 0 {
            self.indent = if flow { BEST_INDENT } else { 0 };
        } else if parent == Parent::SeqItem {
            self.indent += 2;
        } else {
            self.indent = BEST_INDENT * ((self.indent + BEST_INDENT) / BEST_INDENT);
        }
    }

    fn pop_indent(&mut self) {
        self.indent = self.indents.pop().unwrap();
    }

    fn write_indent(&mut self) {
        let indent = self.indent.max(0);
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.put_break();
        }
        while self.column < indent {
            self.put(' ');
        }
        self.whitespace = true;
    }

    fn write_indicator(
        &mut self,
        ind: &str,
        need_whitespace: bool,
        is_whitespace: bool,
        is_indention: bool,
    ) {
        if need_whitespace && !self.whitespace {
            self.put(' ');
        }
        self.write_str(ind);
        self.whitespace = is_whitespace;
        self.indention = self.indention && is_indention;
    }

    fn node(&mut self, n: &Node, parent: Parent, simple_key: bool) {
        match n {
            Node::Scalar(s, style) => self.scalar(s, *style, parent, simple_key),
            Node::Seq(items) if items.is_empty() => self.empty_flow("[", "]", parent),
            Node::Map(entries) if entries.is_empty() => self.empty_flow("{", "}", parent),
            Node::Seq(items) => {
                self.increase_indent(false, parent);
                for item in items {
                    self.write_indent();
                    self.write_indicator("-", true, false, true);
                    self.node(item, Parent::SeqItem, false);
                }
                self.pop_indent();
            }
            Node::Map(entries) => {
                self.increase_indent(false, parent);
                for (k, v) in entries {
                    self.write_indent();
                    self.scalar(k, Style::Str, Parent::MapKey, true);
                    self.write_indicator(":", false, false, false);
                    self.node(v, Parent::MapValue, false);
                }
                self.pop_indent();
            }
        }
    }

    fn empty_flow(&mut self, open: &str, close: &str, parent: Parent) {
        self.write_indicator(open, true, true, false);
        self.increase_indent(true, parent);
        self.pop_indent();
        if self.column == 0 {
            self.write_indent();
        }
        self.write_indicator(close, false, false, false);
    }

    fn scalar(&mut self, s: &str, style: Style, parent: Parent, simple_key: bool) {
        let a = analyze(s);
        let mut chosen = match style {
            Style::Plain => ScalarStyle::Plain,
            Style::Str if s.contains('\n') => ScalarStyle::Literal,
            Style::Str if resolves_to_string(s) => ScalarStyle::Plain,
            Style::Str => ScalarStyle::Double,
        };
        if simple_key && a.multiline {
            chosen = ScalarStyle::Double;
        }
        if chosen == ScalarStyle::Plain && (!a.block_plain_allowed || (s.is_empty() && simple_key))
        {
            chosen = ScalarStyle::Single;
        }
        if chosen == ScalarStyle::Single && !a.single_quoted_allowed {
            chosen = ScalarStyle::Double;
        }
        if chosen == ScalarStyle::Literal && (!a.block_allowed || simple_key) {
            chosen = ScalarStyle::Double;
        }
        self.increase_indent(true, parent);
        match chosen {
            ScalarStyle::Plain => self.write_plain(s),
            ScalarStyle::Single => self.write_single(s),
            ScalarStyle::Double => self.write_double(s),
            ScalarStyle::Literal => self.write_literal(s),
        }
        self.pop_indent();
    }

    fn write_plain(&mut self, s: &str) {
        if !s.is_empty() && !self.whitespace {
            self.put(' ');
        }
        let mut breaks = false;
        for c in s.chars() {
            if is_break(c) {
                if !breaks && c == '\n' {
                    self.put_break();
                }
                self.write_break(c);
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                self.put(c);
                self.indention = false;
                breaks = false;
            }
        }
        if !s.is_empty() {
            self.whitespace = false;
        }
        self.indention = false;
    }

    fn write_break(&mut self, c: char) {
        if c == '\n' {
            self.put_break();
        } else {
            self.out.push(c);
            self.column = 0;
            self.indention = true;
        }
    }

    fn write_single(&mut self, s: &str) {
        self.write_indicator("'", true, false, false);
        let mut breaks = false;
        for c in s.chars() {
            if is_break(c) {
                if !breaks && c == '\n' {
                    self.put_break();
                }
                self.write_break(c);
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                if c == '\'' {
                    self.put('\'');
                }
                self.put(c);
                self.indention = false;
                breaks = false;
            }
        }
        self.write_indicator("'", false, false, false);
        self.whitespace = false;
        self.indention = false;
    }

    fn write_double(&mut self, s: &str) {
        self.write_indicator("\"", true, false, false);
        for (i, c) in s.char_indices() {
            if !is_printable(c)
                || (i == 0 && c == '\u{feff}')
                || is_break(c)
                || c == '"'
                || c == '\\'
            {
                self.put('\\');
                let v = c as u32;
                match v {
                    0x00 => self.put('0'),
                    0x07 => self.put('a'),
                    0x08 => self.put('b'),
                    0x09 => self.put('t'),
                    0x0a => self.put('n'),
                    0x0b => self.put('v'),
                    0x0c => self.put('f'),
                    0x0d => self.put('r'),
                    0x1b => self.put('e'),
                    0x22 => self.put('"'),
                    0x5c => self.put('\\'),
                    0x85 => self.put('N'),
                    0xa0 => self.put('_'),
                    0x2028 => self.put('L'),
                    0x2029 => self.put('P'),
                    _ => {
                        let (tag, width) = if v <= 0xff {
                            ('x', 2)
                        } else if v <= 0xffff {
                            ('u', 4)
                        } else {
                            ('U', 8)
                        };
                        self.put(tag);
                        self.write_str(&format!("{v:0width$X}"));
                    }
                }
            } else {
                self.put(c);
            }
        }
        self.write_indicator("\"", false, false, false);
        self.whitespace = false;
        self.indention = false;
    }

    fn write_literal(&mut self, s: &str) {
        self.write_indicator("|", true, false, false);
        let first = s.chars().next();
        if first.is_some_and(|c| c == ' ' || is_break(c)) {
            self.write_indicator(&BEST_INDENT.to_string(), false, false, false);
        }
        let chars: Vec<char> = s.chars().collect();
        let chomp = match chars.as_slice() {
            [] => Some("-"),
            [.., last] if !is_break(*last) => Some("-"),
            [_] => Some("+"),
            [.., prev, _] if is_break(*prev) => Some("+"),
            _ => None,
        };
        if let Some(h) = chomp {
            self.write_indicator(h, false, false, false);
        }
        self.whitespace = true;
        let mut breaks = true;
        for c in chars {
            if is_break(c) {
                self.write_break(c);
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                self.put(c);
                self.indention = false;
                breaks = false;
            }
        }
    }
}

fn is_break(c: char) -> bool {
    matches!(c, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// libyaml's `is_printable`, which rejects characters outside the BMP.
fn is_printable(c: char) -> bool {
    let v = c as u32;
    v == 0x0a
        || (0x20..=0x7e).contains(&v)
        || (0xa0..=0xd7ff).contains(&v)
        || ((0xe000..=0xfffd).contains(&v) && v != 0xfeff)
}

struct Analysis {
    multiline: bool,
    block_plain_allowed: bool,
    single_quoted_allowed: bool,
    block_allowed: bool,
}

fn analyze(s: &str) -> Analysis {
    if s.is_empty() {
        return Analysis {
            multiline: false,
            block_plain_allowed: true,
            single_quoted_allowed: true,
            block_allowed: false,
        };
    }
    let mut block_indicators = s.starts_with("---") || s.starts_with("...");
    let (mut line_breaks, mut special, mut tabs) = (false, false, false);
    let (mut leading_space, mut leading_break, mut trailing_space, mut trailing_break) =
        (false, false, false, false);
    let (mut break_space, mut space_break) = (false, false);
    let (mut previous_space, mut previous_break) = (false, false);
    let mut preceded_by_whitespace = true;
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let followed_by_whitespace = chars.get(i + 1).is_none_or(|&n| n == ' ' || n == '\t');
        if i == 0 {
            match c {
                '#' | ',' | '[' | ']' | '{' | '}' | '&' | '*' | '!' | '|' | '>' | '\'' | '"'
                | '%' | '@' | '`' => block_indicators = true,
                '?' | ':' if followed_by_whitespace => block_indicators = true,
                '-' if followed_by_whitespace => block_indicators = true,
                _ => {}
            }
        } else {
            match c {
                ':' if followed_by_whitespace => block_indicators = true,
                '#' if preceded_by_whitespace => block_indicators = true,
                _ => {}
            }
        }
        if c == '\t' {
            tabs = true;
        } else if !is_printable(c) {
            special = true;
        }
        let last = i + 1 == chars.len();
        if c == ' ' {
            leading_space |= i == 0;
            trailing_space |= last;
            break_space |= previous_break;
            previous_space = true;
            previous_break = false;
        } else if is_break(c) {
            line_breaks = true;
            leading_break |= i == 0;
            trailing_break |= last;
            space_break |= previous_space;
            previous_space = false;
            previous_break = true;
        } else {
            previous_space = false;
            previous_break = false;
        }
        preceded_by_whitespace = c == ' ' || c == '\t' || is_break(c);
    }
    let mut a = Analysis {
        multiline: line_breaks,
        block_plain_allowed: true,
        single_quoted_allowed: true,
        block_allowed: true,
    };
    if leading_space || leading_break || trailing_space || trailing_break {
        a.block_plain_allowed = false;
    }
    if trailing_space {
        a.block_allowed = false;
    }
    if break_space {
        a.block_plain_allowed = false;
        a.single_quoted_allowed = false;
    }
    if space_break || tabs || special {
        a.block_plain_allowed = false;
        a.single_quoted_allowed = false;
    }
    if space_break || special {
        a.block_allowed = false;
    }
    if line_breaks {
        a.block_plain_allowed = false;
    }
    if block_indicators {
        a.block_plain_allowed = false;
    }
    a
}

/// yaml.v3 `resolve` plus the YAML 1.1 compatibility checks in `stringv`:
/// true when the plain text would read back as the same string.
fn resolves_to_string(s: &str) -> bool {
    let Some(first) = s.bytes().next() else {
        return false;
    };
    let hint = match first {
        b'+' | b'-' => 'S',
        b'0'..=b'9' => 'D',
        b'y' | b'Y' | b'n' | b'N' | b't' | b'T' | b'f' | b'F' | b'o' | b'O' | b'~' => 'M',
        b'.' => '.',
        _ => return !is_base60_float(s) && !is_old_bool(s),
    };
    const SPECIAL: &[&str] = &[
        "true", "True", "TRUE", "false", "False", "FALSE", "~", "null", "Null", "NULL", ".nan",
        ".NaN", ".NAN", ".inf", ".Inf", ".INF", "+.inf", "+.Inf", "+.INF", "-.inf", "-.Inf",
        "-.INF", "<<",
    ];
    if SPECIAL.contains(&s) {
        return false;
    }
    let resolves_other = match hint {
        '.' => s.parse::<f64>().is_ok() && go_float_syntax(s),
        'D' | 'S' => {
            is_timestamp(s) || {
                let plain = s.replace('_', "");
                go_parse_int(&plain)
                    || is_yaml_float(&plain)
                    || plain
                        .strip_prefix("0b")
                        .is_some_and(|d| !d.is_empty() && u64::from_str_radix(d, 2).is_ok())
                    || plain.strip_prefix("-0b").is_some_and(|d| {
                        !d.is_empty() && i64::from_str_radix(&format!("-{d}"), 2).is_ok()
                    })
                    || plain
                        .strip_prefix("0o")
                        .is_some_and(|d| !d.is_empty() && u64::from_str_radix(d, 8).is_ok())
                    || plain.strip_prefix("-0o").is_some_and(|d| {
                        !d.is_empty() && i64::from_str_radix(&format!("-{d}"), 8).is_ok()
                    })
            }
        }
        _ => false,
    };
    !resolves_other && !is_base60_float(s) && !is_old_bool(s)
}

fn go_float_syntax(s: &str) -> bool {
    let t = s.strip_prefix(['+', '-']).unwrap_or(s);
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-' | '_'))
}

/// strconv.ParseInt(s, 0, 64) or ParseUint(s, 0, 64) succeeding, after
/// yaml.v3 has already removed underscores.
fn go_parse_int(s: &str) -> bool {
    let (neg, t) = match s.strip_prefix('-') {
        Some(t) => (true, t),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if t.is_empty() {
        return false;
    }
    let (radix, digits) = if let Some(d) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        (16, d)
    } else if let Some(d) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        (2, d)
    } else if let Some(d) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        (8, d)
    } else if t.len() > 1 && t.starts_with('0') {
        (8, &t[1..])
    } else {
        (10, t)
    };
    if digits.is_empty() {
        return false;
    }
    match u64::from_str_radix(digits, radix) {
        Ok(v) => !neg || v <= 1 << 63,
        Err(_) => false,
    }
}

fn is_yaml_float(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let start = i;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let d = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == d {
            return false;
        }
    } else {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let d = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == d {
            return false;
        }
    }
    i == b.len()
}

fn is_old_bool(s: &str) -> bool {
    matches!(
        s,
        "y" | "Y"
            | "yes"
            | "Yes"
            | "YES"
            | "on"
            | "On"
            | "ON"
            | "n"
            | "N"
            | "no"
            | "No"
            | "NO"
            | "off"
            | "Off"
            | "OFF"
    )
}

/// `^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+(?:\.[0-9_]*)?$`
fn is_base60_float(s: &str) -> bool {
    let b = s.as_bytes();
    let Some(&c) = b.first() else { return false };
    if !(c == b'+' || c == b'-' || c.is_ascii_digit()) || !s.contains(':') {
        return false;
    }
    let mut i = usize::from(c == b'+' || c == b'-');
    if i >= b.len() || !b[i].is_ascii_digit() {
        return false;
    }
    i += 1;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
        i += 1;
    }
    let mut groups = 0;
    while i < b.len() && b[i] == b':' {
        i += 1;
        let d = i;
        while i < b.len() && b[i].is_ascii_digit() && i - d < 2 {
            i += 1;
        }
        let n = &s[d..i];
        let ok = match n.len() {
            1 => true,
            2 => n.as_bytes()[0] <= b'5',
            _ => false,
        };
        if !ok {
            return false;
        }
        groups += 1;
    }
    if groups == 0 {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
            i += 1;
        }
    }
    i == b.len()
}

/// yaml.v3 `parseTimestamp`: Go time.Parse with the layouts
/// "2006-1-2T15:4:5.999999999Z07:00" (T or t), "2006-1-2 15:4:5.999999999"
/// and "2006-1-2".
fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 5 || !b[..4].iter().all(u8::is_ascii_digit) || b[4] != b'-' {
        return false;
    }
    let mut p = Parser { b, i: 5 };
    let Some(month) = p.num(1, 2) else {
        return false;
    };
    if !p.eat(b'-') {
        return false;
    }
    let Some(day) = p.num(1, 2) else { return false };
    let year: u32 = s[..4].parse().unwrap();
    if !(1..=12).contains(&month) || day < 1 || day > days_in(year, month) {
        return false;
    }
    if p.done() {
        return true;
    }
    let sep = b[p.i];
    if !matches!(sep, b'T' | b't' | b' ') {
        return false;
    }
    p.i += 1;
    let (Some(h), true, Some(m), true, Some(sec)) = (
        p.num(1, 2),
        p.eat(b':'),
        p.num(1, 2),
        p.eat(b':'),
        p.num(1, 2),
    ) else {
        return false;
    };
    if h > 23 || m > 59 || sec > 59 {
        return false;
    }
    if p.i < b.len() && b[p.i] == b'.' {
        p.i += 1;
        let d = p.i;
        while p.i < b.len() && b[p.i].is_ascii_digit() {
            p.i += 1;
        }
        if p.i == d {
            return false;
        }
    }
    if sep == b' ' {
        return p.done();
    }
    if p.eat(b'Z') {
        return p.done();
    }
    if p.i < b.len() && (b[p.i] == b'+' || b[p.i] == b'-') {
        p.i += 1;
        let (Some(zh), true, Some(zm)) = (p.num(2, 2), p.eat(b':'), p.num(2, 2)) else {
            return false;
        };
        return zh <= 23 && zm <= 59 && p.done();
    }
    false
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn num(&mut self, min: usize, max: usize) -> Option<u32> {
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() && self.i - start < max {
            self.i += 1;
        }
        if self.i - start < min {
            return None;
        }
        std::str::from_utf8(&self.b[start..self.i])
            .ok()?
            .parse()
            .ok()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.i < self.b.len() && self.b[self.i] == c {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn done(&self) -> bool {
        self.i == self.b.len()
    }
}

fn days_in(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_resolution_matches_yaml_v3() {
        for s in [
            "",
            "true",
            "123",
            "0x1F",
            "1_000",
            "2026-01-12T12:34:50Z",
            "2026-1-2",
            "null",
            "~",
            "yes",
            "1:30",
            ".5",
            "1e3",
        ] {
            assert!(
                !resolves_to_string(s),
                "{s:?} should resolve to a non-string"
            );
        }
        for s in ["id-0", "abc", "2026-13-01", "1.2.3", "@at", "0x", "12a"] {
            assert!(resolves_to_string(s), "{s:?} should stay a string");
        }
    }

    #[test]
    fn floats_use_go_g_format() {
        assert_eq!(format_float(4.857143), "4.857143");
        assert_eq!(format_float(1234567.0), "1.234567e+06");
        assert_eq!(format_float(0.0001), "0.0001");
        assert_eq!(format_float(0.00001), "1e-05");
        assert_eq!(format_float(100000.0), "100000");
    }
}
