//! Go's `encoding/json` v1 `Marshal` of a decoded `map[string]any`: sorted
//! keys, float64 numbers and HTML-safe escaping. ADK sends function call
//! arguments and results to OpenAI in this form, and its REST server writes
//! every response body this way.

use serde_json::{Map, Value};

pub fn marshal(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out, true);
    out
}

/// `json.Marshal` of a Go struct tree built as `v`: keys stay in insertion
/// order (struct field order), so map-typed fields must be inserted sorted,
/// as [`sorted`] does.
pub fn encode(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out, false);
    out
}

/// `json.MarshalIndent(v, "", "  ")` of the same tree: Go's `Indent` over
/// the compact encoding, which keeps empty arrays and objects on one line.
pub fn encode_indent(v: &Value) -> String {
    let compact = encode(v);
    let mut out = String::with_capacity(compact.len() * 2);
    let mut depth = 0usize;
    let (mut in_string, mut escaped) = (false, false);
    let mut chars = compact.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    };
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                let close = if c == '{' { '}' } else { ']' };
                if chars.peek() == Some(&close) {
                    out.push(chars.next().expect("peeked"));
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth -= 1;
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c => out.push(c),
        }
    }
    out
}

/// `v` with every object's keys sorted, as Go writes a `map[string]any`.
pub fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sorted(&m[k])))
                    .collect::<Map<_, _>>(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

fn write(v: &Value, out: &mut String, sort: bool) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            out.push_str(&yutu_api::json::format_float(n.as_f64().unwrap_or(0.0)));
        }
        Value::String(s) => quote(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out, sort);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            if sort {
                keys.sort();
            }
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(k, out);
                out.push(':');
                write(&m[k], out, sort);
            }
            out.push('}');
        }
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' | '>' | '&' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::marshal;
    use serde_json::json;

    #[test]
    fn matches_go_v1_marshal() {
        let v = json!({"z": 1, "a": "<b>&\u{2028}", "n": [1.5, 1e21, 5], "t": true, "o": {}});
        let u = |hex: &str| format!("{}u{hex}", '\\');
        let expected = format!(
            r#"{{"a":"{}b{}{}{}","n":[1.5,1e+21,5],"o":{{}},"t":true,"z":1}}"#,
            u("003c"),
            u("003e"),
            u("0026"),
            u("2028")
        );
        assert_eq!(marshal(&v), expected);
    }
}
