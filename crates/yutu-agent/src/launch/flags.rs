//! Go's `flag` package as ADK's launchers use it: `ContinueOnError` flag
//! sets whose parse errors, usage text and value syntax match Go's.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Str,
    Int,
    /// A `time.Duration`.
    Duration,
}

pub struct Flag {
    pub name: &'static str,
    pub kind: Kind,
    /// The default as Go's `DefValue` prints it.
    pub default: &'static str,
    pub usage: &'static str,
}

pub struct FlagSet {
    pub name: &'static str,
    pub flags: &'static [Flag],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Str(String),
    Int(i64),
    /// Nanoseconds, which may be negative.
    Duration(i64),
}

/// A parsed flag set: every flag's value, set or default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Values(BTreeMap<&'static str, Value>);

impl Values {
    pub fn bool(&self, name: &str) -> bool {
        matches!(self.0.get(name), Some(Value::Bool(true)))
    }

    pub fn str(&self, name: &str) -> String {
        match self.0.get(name) {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        }
    }

    pub fn int(&self, name: &str) -> i64 {
        match self.0.get(name) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        }
    }

    pub fn nanos(&self, name: &str) -> i64 {
        match self.0.get(name) {
            Some(Value::Duration(n)) => *n,
            _ => 0,
        }
    }

    /// A server timeout: Go treats zero and negative as none.
    pub fn timeout(&self, name: &str) -> Option<Duration> {
        u64::try_from(self.nanos(name))
            .ok()
            .filter(|n| *n > 0)
            .map(Duration::from_nanos)
    }
}

impl FlagSet {
    /// `flag.FlagSet.Parse`: the values and the arguments after the flags.
    /// Errors print Go's message and usage to `stderr`.
    pub fn parse(
        &self,
        args: &[String],
        stderr: &mut dyn Write,
    ) -> Result<(Values, Vec<String>), String> {
        let mut values = BTreeMap::new();
        for f in self.flags {
            values.insert(f.name, set(f, f.default).expect("valid default"));
        }
        let fail = |stderr: &mut dyn Write, msg: String, help: bool| {
            if !help {
                let _ = writeln!(stderr, "{msg}");
            }
            let _ = write!(stderr, "Usage of {}:\n{}", self.name, self.defaults());
            msg
        };
        let mut i = 0;
        while i < args.len() {
            let s = &args[i];
            if s.len() < 2 || !s.starts_with('-') {
                break;
            }
            let mut name = &s[1..];
            if let Some(n) = name.strip_prefix('-') {
                if n.is_empty() {
                    i += 1;
                    break;
                }
                name = n;
            }
            if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
                return Err(fail(stderr, format!("bad flag syntax: {s}"), false));
            }
            i += 1;
            let (name, value) = match name.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (name, None),
            };
            let Some(flag) = self.flags.iter().find(|f| f.name == name) else {
                if name == "help" || name == "h" {
                    return Err(fail(stderr, "flag: help requested".into(), true));
                }
                return Err(fail(
                    stderr,
                    format!("flag provided but not defined: -{name}"),
                    false,
                ));
            };
            let value = match (flag.kind, value) {
                (Kind::Bool, v) => v.unwrap_or_else(|| "true".into()),
                (_, Some(v)) => v,
                (_, None) if i < args.len() => {
                    i += 1;
                    args[i - 1].clone()
                }
                (_, None) => {
                    return Err(fail(
                        stderr,
                        format!("flag needs an argument: -{name}"),
                        false,
                    ));
                }
            };
            match set(flag, &value) {
                Ok(v) => {
                    values.insert(flag.name, v);
                }
                Err(e) => {
                    let quoted = yutu_auth::log::quote(&value);
                    let msg = if flag.kind == Kind::Bool {
                        format!("invalid boolean value {quoted} for -{name}: {e}")
                    } else {
                        format!("invalid value {quoted} for flag -{name}: {e}")
                    };
                    return Err(fail(stderr, msg, false));
                }
            }
        }
        Ok((Values(values), args[i..].to_vec()))
    }

    /// `flag.FlagSet.PrintDefaults`, flags in name order.
    pub fn defaults(&self) -> String {
        let mut flags: Vec<&Flag> = self.flags.iter().collect();
        flags.sort_by_key(|f| f.name);
        let mut out = String::new();
        for f in flags {
            let mut line = format!("  -{}", f.name);
            let type_name = match f.kind {
                Kind::Bool => "",
                Kind::Str => "string",
                Kind::Int => "int",
                Kind::Duration => "duration",
            };
            if !type_name.is_empty() {
                line.push(' ');
                line.push_str(type_name);
            }
            line.push_str(if line.len() <= 4 { "\t" } else { "\n    \t" });
            line.push_str(&f.usage.replace('\n', "\n    \t"));
            let zero = match f.kind {
                Kind::Bool => "false",
                Kind::Str => "",
                Kind::Int => "0",
                Kind::Duration => "0s",
            };
            if f.default != zero {
                if f.kind == Kind::Str {
                    line.push_str(&format!(" (default {})", yutu_auth::log::quote(f.default)));
                } else {
                    line.push_str(&format!(" (default {})", f.default));
                }
            }
            out.push_str(&line);
            out.push('\n');
        }
        out
    }
}

fn set(flag: &Flag, s: &str) -> Result<Value, &'static str> {
    match flag.kind {
        Kind::Bool => parse_bool(s).map(Value::Bool).ok_or("parse error"),
        Kind::Str => Ok(Value::Str(s.to_string())),
        Kind::Int => parse_int(s).map(Value::Int),
        Kind::Duration => parse_duration(s).map(Value::Duration).ok_or("parse error"),
    }
}

/// Go's `strconv.ParseBool`.
pub fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "true" | "TRUE" | "True" => Some(true),
        "0" | "f" | "F" | "false" | "FALSE" | "False" => Some(false),
        _ => None,
    }
}

/// Go's `strconv.ParseInt(s, 0, 64)`, with its two error texts.
pub fn parse_int(s: &str) -> Result<i64, &'static str> {
    let (neg, body) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let lower = body.to_ascii_lowercase();
    let (base, digits) = if let Some(d) = lower.strip_prefix("0x") {
        (16, d)
    } else if let Some(d) = lower.strip_prefix("0o") {
        (8, d)
    } else if let Some(d) = lower.strip_prefix("0b") {
        (2, d)
    } else if lower.len() > 1 && lower.starts_with('0') {
        (8, &lower[1..])
    } else {
        (10, lower.as_str())
    };
    if digits.is_empty() || (digits.contains('_') && !underscores_ok(&lower)) {
        return Err("parse error");
    }
    let mut n: u128 = 0;
    for c in digits.chars().filter(|c| *c != '_') {
        let d = c.to_digit(base).ok_or("parse error")?;
        n = n * u128::from(base) + u128::from(d);
        if n > 1 << 64 {
            n = 1 << 64;
        }
    }
    let limit = if neg { 1u128 << 63 } else { (1u128 << 63) - 1 };
    if n > limit {
        return Err("value out of range");
    }
    let n = n as i128;
    Ok((if neg { -n } else { n }) as i64)
}

/// strconv's `underscoreOK`: underscores only between digits, or after a
/// base prefix.
fn underscores_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let mut saw = '^';
    let mut i = 0;
    let mut hex = false;
    if b.len() >= 2 && b[0] == b'0' && matches!(b[1], b'x' | b'b' | b'o') {
        i = 2;
        saw = '0';
        hex = b[1] == b'x';
    }
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || (hex && c.is_ascii_hexdigit()) {
            saw = '0';
        } else if c == b'_' {
            if saw != '0' {
                return false;
            }
            saw = '_';
        } else {
            if saw == '_' {
                return false;
            }
            saw = '!';
        }
        i += 1;
    }
    saw != '_'
}

/// Go's `time.ParseDuration`, in nanoseconds.
pub fn parse_duration(s: &str) -> Option<i64> {
    let (neg, mut rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if rest == "0" {
        return Some(0);
    }
    if rest.is_empty() {
        return None;
    }
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let int_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let int_part = &rest[..int_end];
        rest = &rest[int_end..];
        let mut frac_part = "";
        if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
            frac_part = &r[..end];
            rest = &r[end..];
            if int_part.is_empty() && frac_part.is_empty() {
                return None;
            }
        } else if int_part.is_empty() {
            return None;
        }
        let unit_end = rest
            .find(|c: char| c.is_ascii_digit() || c == '.')
            .unwrap_or(rest.len());
        let scale: u128 = match &rest[..unit_end] {
            "ns" => 1,
            "us" | "µs" | "μs" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return None,
        };
        rest = &rest[unit_end..];
        let whole: u128 = if int_part.is_empty() {
            0
        } else {
            int_part.parse().ok()?
        };
        let mut v = whole.checked_mul(scale)?;
        let mut f_scale = scale;
        for d in frac_part.bytes() {
            f_scale /= 10;
            if f_scale == 0 {
                break;
            }
            v += u128::from(d - b'0') * f_scale;
        }
        total = total.checked_add(v)?;
        if total > 1 << 63 {
            return None;
        }
    }
    if neg {
        Some((-(total as i128)) as i64)
    } else if total > i64::MAX as u128 {
        None
    } else {
        Some(total as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_parse_like_strconv() {
        assert_eq!(parse_int("8080"), Ok(8080));
        assert_eq!(parse_int("0x1F"), Ok(31));
        assert_eq!(parse_int("010"), Ok(8));
        assert_eq!(parse_int("-0b11"), Ok(-3));
        assert_eq!(parse_int("1_000"), Ok(1000));
        assert_eq!(parse_int("1__0"), Err("parse error"));
        assert_eq!(parse_int("0x_1_0"), Ok(16));
        assert_eq!(parse_int("x"), Err("parse error"));
        assert_eq!(parse_int("9223372036854775808"), Err("value out of range"));
        assert_eq!(parse_int("-9223372036854775808"), Ok(i64::MIN));
    }

    #[test]
    fn durations_parse_like_time() {
        assert_eq!(parse_duration("15s"), Some(15_000_000_000));
        assert_eq!(parse_duration("1m30s"), Some(90_000_000_000));
        assert_eq!(parse_duration("1.5h"), Some(5_400_000_000_000));
        assert_eq!(parse_duration("-2ms"), Some(-2_000_000));
        assert_eq!(parse_duration(".5s"), Some(500_000_000));
        assert_eq!(parse_duration("0"), Some(0));
        assert_eq!(parse_duration("10"), None);
        assert_eq!(parse_duration("1x"), None);
        assert_eq!(parse_duration("."), None);
    }
}
