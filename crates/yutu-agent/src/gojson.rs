//! Go's `encoding/json` v1 `Marshal` of a decoded `map[string]any`: sorted
//! keys, float64 numbers and HTML-safe escaping. ADK sends function call
//! arguments and results to OpenAI in this form.

use serde_json::Value;

pub fn marshal(v: &Value) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Value, out: &mut String) {
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
                write(item, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                quote(k, out);
                out.push(':');
                write(&m[k], out);
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
