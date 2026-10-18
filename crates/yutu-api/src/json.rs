//! Encodes [`Value`] the way yutu's Go output does: each struct goes through
//! gensupport.MarshalJSON (encoding/json v1 with omitempty, ForceSendFields
//! and NullFields), and the result is re-emitted by encoding/json/v2, which
//! drops v1's HTML escaping and indents only when asked.

use crate::schema::{Field, Kind, type_def};
use crate::value::{Struct, Value};

/// An ordered JSON tree with numbers kept as the literal text Go wrote.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Go's json/v2 encoding of `[]*T`, where a nil slice is `[]`.
pub fn list(items: &[Value], indent: bool) -> String {
    let mut out = String::with_capacity(items.len() * 1024);
    if items.is_empty() {
        out.push_str("[]");
        return out;
    }
    out.push('[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        newline(indent, 1, &mut out);
        write_element(item, indent, 1, &mut out);
    }
    newline(indent, 0, &mut out);
    out.push(']');
    out
}

/// One element of a streamed `[]*T` encoding: the separator and the item.
/// Writing every item this way, then [`list_end`], gives [`list`]'s bytes.
pub fn list_item(item: &Value, indent: bool, first: bool, out: &mut String) {
    out.push_str(if first { "[" } else { "," });
    newline(indent, 1, out);
    write_element(item, indent, 1, out);
}

/// Closes a streamed list of `count` items.
pub fn list_end(indent: bool, count: usize, out: &mut String) {
    if count == 0 {
        out.push_str("[]");
    } else {
        newline(indent, 0, out);
        out.push(']');
    }
}

/// Go's json/v2 encoding of one `*T` result.
pub fn value(v: &Value, indent: bool) -> String {
    let mut out = String::new();
    write_element(v, indent, 0, &mut out);
    out
}

/// The request body Go sends for a struct, before transport.
pub fn body(v: &Value) -> String {
    value(v, false)
}

/// Writes one element straight to the output. Structs without
/// ForceSendFields or NullFields keep Go field order and need no
/// intermediate tree; the rare forced case goes through [`Json`].
fn write_element(v: &Value, indent: bool, depth: usize, out: &mut String) {
    match v {
        Value::Struct(s)
            if s.go_strings("ForceSendFields").is_empty()
                && s.go_strings("NullFields").is_empty() =>
        {
            let def = type_def(s.ty);
            let mut first = true;
            out.push('{');
            for (f, v) in def.fields.iter().zip(&s.fields) {
                if f.skip || (f.omitempty && is_empty(f.kind, v)) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                newline(indent, depth + 1, out);
                quote(f.json, out);
                out.push_str(if indent { ": " } else { ":" });
                write_field(v, f, indent, depth + 1, out);
            }
            if !first {
                newline(indent, depth, out);
            }
            out.push('}');
        }
        other => emit(&element(other), indent, depth, out),
    }
}

fn write_field(v: &Value, f: &Field, indent: bool, depth: usize, out: &mut String) {
    match v {
        Value::Str(s) if !f.string => quote(s, out),
        Value::Struct(_) => write_element(v, indent, depth, out),
        Value::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(indent, depth + 1, out);
                write_element(item, indent, depth + 1, out);
            }
            newline(indent, depth, out);
            out.push(']');
        }
        _ => emit(&plain(v, Some(f)), indent, depth, out),
    }
}

fn element(v: &Value) -> Json {
    match v {
        Value::Struct(s) => marshal_struct(s),
        Value::Nil => Json::Null,
        other => plain(other, None),
    }
}

fn is_empty(kind: Kind, v: &Value) -> bool {
    if matches!(kind, Kind::PtrInt64 | Kind::PtrBool) {
        return v.is_nil();
    }
    match v {
        Value::Nil => true,
        Value::Bool(b) => !b,
        Value::Int(i) => *i == 0,
        Value::Uint(u) => *u == 0,
        Value::Float(f) => *f == 0.0,
        Value::Str(s) => s.is_empty(),
        Value::Strings(v) => v.is_empty(),
        Value::Header(v) => v.is_empty(),
        Value::List(v) => v.is_empty(),
        Value::Map(v) => v.is_empty(),
        Value::Struct(_) => false,
    }
}

fn marshal_struct(s: &Struct) -> Json {
    let def = type_def(s.ty);
    let force = s.go_strings("ForceSendFields");
    let nulls = s.go_strings("NullFields");
    if force.is_empty() && nulls.is_empty() {
        let mut out = Vec::new();
        for (f, v) in def.fields.iter().zip(&s.fields) {
            if f.skip || (f.omitempty && is_empty(f.kind, v)) {
                continue;
            }
            out.push((f.json.to_string(), plain(v, Some(f))));
        }
        return Json::Object(out);
    }
    let mut out = Vec::new();
    for (f, v) in def.fields.iter().zip(&s.fields) {
        if f.skip {
            continue;
        }
        if nulls.iter().any(|n| n == f.go) {
            out.push((f.json.to_string(), Json::Null));
            continue;
        }
        let pointer = matches!(f.kind, Kind::Ptr(_) | Kind::PtrInt64 | Kind::PtrBool);
        if pointer && v.is_nil() {
            continue;
        }
        if !force.iter().any(|n| n == f.go) && is_empty(f.kind, v) {
            continue;
        }
        let j = match (f.kind, v) {
            (Kind::Map(_) | Kind::Header, Value::Nil) => Json::Object(Vec::new()),
            (Kind::Strings | Kind::PtrSlice(_), Value::Nil) => Json::Array(Vec::new()),
            _ if f.string => Json::Str(number(v)),
            _ => plain(v, Some(f)),
        };
        out.push((f.json.to_string(), j));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Json::Object(out)
}

fn number(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Uint(u) => u.to_string(),
        Value::Float(f) => format_float(*f),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        _ => String::new(),
    }
}

fn plain(v: &Value, f: Option<&Field>) -> Json {
    let string_tag = f.is_some_and(|f| f.string);
    match v {
        Value::Nil => Json::Null,
        Value::Bool(b) if string_tag => Json::Str(b.to_string()),
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(_) | Value::Uint(_) | Value::Float(_) if string_tag => Json::Str(number(v)),
        Value::Int(_) | Value::Uint(_) => Json::Number(number(v)),
        Value::Float(f) if f.is_finite() => Json::Number(format_float(*f)),
        Value::Float(f) => Json::Str(
            if f.is_nan() {
                "NaN"
            } else if *f > 0.0 {
                "Infinity"
            } else {
                "-Infinity"
            }
            .into(),
        ),
        Value::Str(s) => Json::Str(s.clone()),
        Value::Strings(v) => Json::Array(v.iter().map(|s| Json::Str(s.clone())).collect()),
        Value::Header(h) => {
            let mut h = h.clone();
            h.sort_by(|a, b| a.0.cmp(&b.0));
            Json::Object(
                h.into_iter()
                    .map(|(k, v)| (k, Json::Array(v.into_iter().map(Json::Str).collect())))
                    .collect(),
            )
        }
        Value::Struct(s) => marshal_struct(s),
        Value::List(items) => Json::Array(items.iter().map(element).collect()),
        Value::Map(m) => {
            let mut m: Vec<_> = m.iter().collect();
            m.sort_by(|a, b| a.0.cmp(&b.0));
            Json::Object(
                m.into_iter()
                    .map(|(k, v)| (k.clone(), element(v)))
                    .collect(),
            )
        }
    }
}

/// encoding/json v1 float64 formatting.
pub fn format_float(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let abs = f.abs();
    if !(1e-6..1e21).contains(&abs) {
        let s = format!("{f:e}");
        let (mant, exp) = s.split_once('e').unwrap();
        let exp: i32 = exp.parse().unwrap();
        let sign = if exp < 0 { '-' } else { '+' };
        let digits = exp.abs();
        return if digits < 10 {
            format!("{mant}e{sign}0{digits}")
        } else {
            format!("{mant}e{sign}{digits}")
        }
        .replace("e-0", "e-");
    }
    let s = format!("{f}");
    s
}

pub fn write(j: &Json, indent: bool) -> String {
    let mut out = String::new();
    emit(j, indent, 0, &mut out);
    out
}

fn emit(j: &Json, indent: bool, depth: usize, out: &mut String) {
    match j {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => out.push_str(n),
        Json::Str(s) => quote(s, out),
        Json::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, e) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(indent, depth + 1, out);
                emit(e, indent, depth + 1, out);
            }
            newline(indent, depth, out);
            out.push(']');
        }
        Json::Object(fields) => {
            if fields.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, v)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(indent, depth + 1, out);
                quote(k, out);
                out.push_str(if indent { ": " } else { ":" });
                emit(v, indent, depth + 1, out);
            }
            newline(indent, depth, out);
            out.push('}');
        }
    }
}

fn newline(indent: bool, depth: usize, out: &mut String) {
    if indent {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    }
}

/// encoding/json/v2 string quoting: only `"`, `\` and control characters
/// are escaped.
pub fn quote(s: &str, out: &mut String) {
    out.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc = match b {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..0x20 => "",
            _ => continue,
        };
        out.push_str(&s[start..i]);
        if esc.is_empty() {
            out.push_str(&format!("\\u{b:04x}"));
        } else {
            out.push_str(esc);
        }
        start = i + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}
