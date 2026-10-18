//! The part of google/jsonschema-go v0.4.3 that go-sdk applies to tool
//! arguments: defaults first, then validation of `type`, `enum`, `minimum`,
//! `items`, `properties` and `required`, with the same error texts. Go walks
//! `properties` in map order, so when several properties fail it reports a
//! random one; this walks them in schema order.

use crate::log::go_quote;
use serde_json::{Map, Number, Value};

/// go-sdk's `applySchema` for tool input: the arguments (JSON text) as an
/// object with defaults filled in, or the error text that follows
/// `validating "arguments": `.
pub fn apply(schema: &Value, args: Option<&str>) -> Result<Value, String> {
    let mut obj = match args.map(serde_json::from_str::<Value>) {
        None | Some(Ok(Value::Null)) => Map::new(),
        Some(Ok(Value::Object(m))) => m,
        _ => {
            return Err(format!(
                "unmarshaling arguments: json: cannot unmarshal {} into Go value of type map[string]interface {{}}",
                go_quote(args.unwrap_or_default())
            ));
        }
    };
    apply_defaults(&mut obj, schema);
    let instance = Value::Object(obj);
    validate(&instance, schema, "", true)?;
    Ok(integral_floats_as_ints(instance))
}

fn apply_defaults(obj: &mut Map<String, Value>, schema: &Value) {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let required = required(schema);
    for (name, sub) in props {
        if required.contains(&name.as_str()) {
            continue;
        }
        match obj.get_mut(name) {
            Some(Value::Object(child)) => apply_defaults(child, sub),
            Some(_) => {}
            None => {
                if let Some(d) = sub.get("default") {
                    let mut d = d.clone();
                    if let Value::Object(child) = &mut d {
                        apply_defaults(child, sub);
                    }
                    obj.insert(name.clone(), d);
                } else if has_nested_defaults(sub) {
                    let mut child = Map::new();
                    apply_defaults(&mut child, sub);
                    obj.insert(name.clone(), Value::Object(child));
                }
            }
        }
    }
}

fn has_nested_defaults(s: &Value) -> bool {
    s.get("default").is_some()
        || s.get("properties")
            .and_then(Value::as_object)
            .is_some_and(|p| p.values().any(has_nested_defaults))
}

fn required(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Go decodes arguments into float64 and re-marshals them, so `5.0` reaches
/// the op as `5`; serde would refuse it for an integer field.
fn integral_floats_as_ints(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => {
                Value::Number(Number::from(f as i64))
            }
            _ => Value::Number(n),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(integral_floats_as_ints).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (k, integral_floats_as_ints(v)))
                .collect(),
        ),
        other => other,
    }
}

fn validate(v: &Value, schema: &Value, pointer: &str, top: bool) -> Result<(), String> {
    check(v, schema, pointer, top).map_err(|e| {
        let at = if pointer.is_empty() { "root" } else { pointer };
        format!("validating {at}: {e}")
    })
}

fn check(v: &Value, schema: &Value, pointer: &str, top: bool) -> Result<(), String> {
    if let Some(want) = schema.get("type").and_then(Value::as_str) {
        let got = json_type(v);
        if !(got == want || got == "integer" && want == "number") {
            return Err(format!(
                "type: {} has type {}, want {}",
                go_value(v, top),
                go_quote(got),
                go_quote(want)
            ));
        }
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array)
        && !choices.iter().any(|c| equal(c, v))
    {
        return Err(format!(
            "enum: {} does not equal any of: {}",
            go_value(v, top),
            go_value(&Value::Array(choices.clone()), false)
        ));
    }
    if let (Some(min), Some(n)) = (
        schema.get("minimum").and_then(Value::as_f64),
        v.as_number().and_then(Number::as_f64),
    ) && n < min
    {
        return Err(format!("minimum: {} is less than {min:.6}", rat(n)));
    }
    if let (Value::Array(items), Some(item_schema)) = (v, schema.get("items")) {
        let at = format!("{pointer}/items");
        for item in items {
            validate(item, item_schema, &at, false)?;
        }
    }
    if let Value::Object(obj) = v {
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (name, sub) in props {
                if let Some(pv) = obj.get(name) {
                    let at = format!("{pointer}/properties/{}", pointer_segment(name));
                    validate(pv, sub, &at, true)?;
                }
            }
        }
        let missing: Vec<&str> = required(schema)
            .into_iter()
            .filter(|p| !obj.contains_key(*p))
            .collect();
        if !missing.is_empty() {
            let quoted: Vec<String> = missing.iter().map(|m| go_quote(m)).collect();
            return Err(format!(
                "required: missing properties: [{}]",
                quoted.join(" ")
            ));
        }
    }
    Ok(())
}

fn pointer_segment(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() != 0.0 => "number",
            _ => "integer",
        },
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equal(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, a)| y.get(k).is_some_and(|b| equal(a, b)))
        }
        _ => a == b,
    }
}

/// `fmt.Sprintf("%v", reflect.Value)` for a decoded JSON value. A null held
/// directly by the validator is an invalid reflect.Value; nested, it is `<nil>`.
fn go_value(v: &Value, top: bool) -> String {
    match v {
        Value::Null if top => "<invalid reflect.Value>".into(),
        Value::Null => "<nil>".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => go_float(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => s.clone(),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(|x| go_value(x, false)).collect();
            format!("[{}]", parts.join(" "))
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{k}:{}", go_value(&m[k], false)))
                .collect();
            format!("map[{}]", parts.join(" "))
        }
    }
}

/// `%v` of a float64, which is `%g` with the shortest digits: exponent form
/// when the decimal exponent is below -4 or at least 6.
fn go_float(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let sci = format!("{f:e}");
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-4..6).contains(&exp) {
        return format!("{f}");
    }
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mant}e{sign}{:02}", exp.abs())
}

/// `big.Rat.SetFloat64(f).String()`: the exact value as `num/den` in lowest terms.
fn rat(f: f64) -> String {
    if f == 0.0 {
        return "0/1".into();
    }
    let bits = f.to_bits();
    let neg = bits >> 63 == 1;
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    let (mut mant, mut exp) = if raw_exp == 0 {
        (frac, -1074)
    } else {
        (frac | (1 << 52), raw_exp - 1075)
    };
    while mant & 1 == 0 && exp < 0 {
        mant >>= 1;
        exp += 1;
    }
    let sign = if neg { "-" } else { "" };
    if exp >= 0 {
        return match (mant as u128).checked_shl(exp as u32) {
            Some(n) if exp < 75 => format!("{sign}{n}/1"),
            _ => format!("{sign}{f}/1"),
        };
    }
    if exp < -127 {
        return format!("{f}");
    }
    format!("{sign}{mant}/{}", 1u128 << (-exp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn video_list() -> Value {
        json!({"type":"object","properties":{
            "ids":{"type":"array","items":{"type":"string"}},
            "max_results":{"type":"number","minimum":0,"default":5},
            "output":{"type":"string","enum":["json","yaml","table"],"default":"json"},
            "parts":{"type":"array","items":{"type":"string"},"default":["id","snippet"]}}})
    }

    #[test]
    fn fills_defaults() {
        let got = apply(&video_list(), Some(r#"{"ids":["a"]}"#)).unwrap();
        assert_eq!(
            got,
            json!({"ids":["a"],"max_results":5,"output":"json","parts":["id","snippet"]})
        );
    }

    #[test]
    fn go_error_texts() {
        let s = video_list();
        let e = |v: Value| apply(&s, Some(&v.to_string())).unwrap_err();
        assert_eq!(
            e(json!({"max_results":"x"})),
            r#"validating root: validating /properties/max_results: type: x has type "string", want "number""#
        );
        assert_eq!(
            e(json!({"output":"xml"})),
            "validating root: validating /properties/output: enum: xml does not equal any of: [json yaml table]"
        );
        assert_eq!(
            e(json!({"max_results":-1})),
            "validating root: validating /properties/max_results: minimum: -1/1 is less than 0.000000"
        );
        assert_eq!(
            e(json!({"ids":["a",1]})),
            r#"validating root: validating /properties/ids: validating /properties/ids/items: type: 1 has type "integer", want "string""#
        );
        assert_eq!(
            e(json!({"ids":null})),
            r#"validating root: validating /properties/ids: type: <invalid reflect.Value> has type "null", want "array""#
        );
        assert_eq!(
            e(json!({"output":["a",null,{"k":2.5}]})),
            r#"validating root: validating /properties/output: type: [a <nil> map[k:2.5]] has type "array", want "string""#
        );
        assert_eq!(
            apply(&json!({"type":"object","required":["a","b"]}), None).unwrap_err(),
            r#"validating root: required: missing properties: ["a" "b"]"#
        );
        assert_eq!(
            apply(&s, Some("[1, 2]")).unwrap_err(),
            r#"unmarshaling arguments: json: cannot unmarshal "[1, 2]" into Go value of type map[string]interface {}"#
        );
    }

    #[test]
    fn integral_floats_reach_ops_as_ints() {
        let got = apply(&video_list(), Some(r#"{"max_results":7.0}"#)).unwrap();
        assert_eq!(got["max_results"], json!(7));
        assert!(got["max_results"].is_i64());
    }

    #[test]
    fn go_number_formatting() {
        assert_eq!(go_float(1.5), "1.5");
        assert_eq!(go_float(100000000.0), "1e+08");
        assert_eq!(go_float(123456.0), "123456");
        assert_eq!(go_float(1234567.0), "1.234567e+06");
        assert_eq!(go_float(0.0001), "0.0001");
        assert_eq!(go_float(0.00001), "1e-05");
        assert_eq!(rat(-0.5), "-1/2");
        assert_eq!(rat(-3.0), "-3/1");
    }
}
