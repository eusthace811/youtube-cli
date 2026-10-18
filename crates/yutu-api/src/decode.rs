//! Decodes API JSON into [`Value`] with encoding/json v1 rules: keys match
//! field tags exactly or case-insensitively, unknown keys are dropped, and a
//! JSON null leaves value fields untouched and pointers nil. Decoding streams
//! straight into the typed tree, without an intermediate JSON document.

use crate::schema::{Kind, type_def};
use crate::value::{Struct, Value};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value as Json;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct DecodeError(pub String);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DecodeError {}

pub fn decode(ty: usize, bytes: &[u8]) -> Result<Value, DecodeError> {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let s = StructSeed { ty }
        .deserialize(&mut de)
        .map_err(|e| DecodeError(clean(e)))?;
    de.end().map_err(|e| DecodeError(clean(e)))?;
    Ok(Value::Struct(Box::new(s)))
}

/// Decodes a JSON value that is already parsed, for MCP tool inputs.
pub fn from_json(ty: usize, json: &Json) -> Result<Value, DecodeError> {
    let s = StructSeed { ty }
        .deserialize(json)
        .map_err(|e| DecodeError(clean(e)))?;
    Ok(Value::Struct(Box::new(s)))
}

fn clean(e: impl fmt::Display) -> String {
    let s = e.to_string();
    match s.find(" at line ") {
        Some(i) if s.starts_with("json: ") => s[..i].to_string(),
        _ if s.contains("EOF while parsing") => "unexpected end of JSON input".into(),
        _ => s,
    }
}

fn go_type(kind: Kind) -> String {
    match kind {
        Kind::String => "string".into(),
        Kind::Bool | Kind::PtrBool => "bool".into(),
        Kind::Int => "int".into(),
        Kind::Int64 | Kind::PtrInt64 => "int64".into(),
        Kind::Uint64 => "uint64".into(),
        Kind::Float64 => "float64".into(),
        Kind::Strings => "string".into(),
        Kind::Header => "[]string".into(),
        Kind::Ptr(t) | Kind::Inline(t) | Kind::PtrSlice(t) | Kind::Map(t) => {
            format!("youtube.{}", type_def(t).name)
        }
    }
}

/// Each type's JSON field names, sorted, for exact-match lookup.
fn field_index(ty: usize) -> &'static [(&'static str, usize)] {
    static INDEX: std::sync::OnceLock<Vec<Vec<(&'static str, usize)>>> = std::sync::OnceLock::new();
    &INDEX.get_or_init(|| {
        crate::generated::TYPES
            .iter()
            .map(|t| {
                let mut v: Vec<_> = t
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| !f.skip)
                    .map(|(i, f)| (f.json, i))
                    .collect();
                v.sort();
                v
            })
            .collect()
    })[ty]
}

struct StructSeed {
    ty: usize,
}

impl<'de> DeserializeSeed<'de> for StructSeed {
    type Value = Struct;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Struct, D::Error> {
        d.deserialize_any(StructVisitor { ty: self.ty })
    }
}

struct StructVisitor {
    ty: usize,
}

impl<'de> Visitor<'de> for StructVisitor {
    type Value = Struct;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "youtube.{}", type_def(self.ty).name)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Struct, E> {
        Ok(Struct::new(self.ty))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Struct, A::Error> {
        let mut s = Struct::new(self.ty);
        let fields = type_def(self.ty).fields;
        let index = field_index(self.ty);
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            let idx = index
                .binary_search_by(|(name, _)| (*name).cmp(&key))
                .ok()
                .map(|i| index[i].1)
                .or_else(|| {
                    fields
                        .iter()
                        .position(|f| !f.skip && f.json.eq_ignore_ascii_case(&key))
                });
            let Some(i) = idx else {
                map.next_value::<IgnoredAny>()?;
                continue;
            };
            let f = &fields[i];
            if let Some(v) = map.next_value_seed(FieldSeed {
                kind: f.kind,
                string: f.string,
                owner: self.ty,
                field: f.json,
            })? {
                s.fields[i] = v;
            } else if matches!(
                f.kind,
                Kind::Ptr(_)
                    | Kind::PtrSlice(_)
                    | Kind::Map(_)
                    | Kind::Strings
                    | Kind::Header
                    | Kind::PtrInt64
                    | Kind::PtrBool
            ) {
                s.fields[i] = Value::Nil;
            }
        }
        Ok(s)
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Struct, E> {
        Err(E::custom(format!(
            "json: cannot unmarshal string into Go value of type youtube.{}",
            type_def(self.ty).name
        )))
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Struct, E> {
        Err(E::custom(format!(
            "json: cannot unmarshal bool into Go value of type youtube.{}",
            type_def(self.ty).name
        )))
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Struct, E> {
        Err(E::custom(format!(
            "json: cannot unmarshal number into Go value of type youtube.{}",
            type_def(self.ty).name
        )))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Struct, E> {
        self.visit_f64(v as f64)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Struct, E> {
        self.visit_f64(v as f64)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, _: A) -> Result<Struct, A::Error> {
        Err(de::Error::custom(format!(
            "json: cannot unmarshal array into Go value of type youtube.{}",
            type_def(self.ty).name
        )))
    }
}

/// Decodes one field; `None` means JSON null.
struct FieldSeed {
    kind: Kind,
    string: bool,
    owner: usize,
    field: &'static str,
}

impl FieldSeed {
    fn mismatch<E: de::Error>(&self, json_kind: &str) -> E {
        E::custom(format!(
            "json: cannot unmarshal {json_kind} into Go struct field {}.{} of type {}",
            type_def(self.owner).name,
            self.field,
            go_type(self.kind)
        ))
    }

    fn number<E: de::Error>(&self, text: &str) -> Result<Option<Value>, E> {
        Ok(Some(match self.kind {
            Kind::Int | Kind::Int64 | Kind::PtrInt64 => {
                Value::Int(text.parse().map_err(|_| self.mismatch::<E>("number"))?)
            }
            Kind::Uint64 => Value::Uint(text.parse().map_err(|_| self.mismatch::<E>("number"))?),
            Kind::Float64 => Value::Float(text.parse().map_err(|_| self.mismatch::<E>("number"))?),
            _ => return Err(self.mismatch("number")),
        }))
    }
}

impl<'de> DeserializeSeed<'de> for FieldSeed {
    type Value = Option<Value>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Option<Value>, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for FieldSeed {
    type Value = Option<Value>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&go_type(self.kind))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Option<Value>, E> {
        Ok(None)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Option<Value>, E> {
        match self.kind {
            Kind::Bool | Kind::PtrBool => Ok(Some(Value::Bool(v))),
            _ => Err(self.mismatch("bool")),
        }
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Option<Value>, E> {
        if self.string {
            return Err(self.mismatch("number"));
        }
        match self.kind {
            Kind::Int | Kind::Int64 | Kind::PtrInt64 => Ok(Some(Value::Int(v))),
            Kind::Uint64 if v >= 0 => Ok(Some(Value::Uint(v as u64))),
            Kind::Float64 => Ok(Some(Value::Float(v as f64))),
            _ => Err(self.mismatch("number")),
        }
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Option<Value>, E> {
        if self.string {
            return Err(self.mismatch("number"));
        }
        match self.kind {
            Kind::Int | Kind::Int64 | Kind::PtrInt64 if v <= i64::MAX as u64 => {
                Ok(Some(Value::Int(v as i64)))
            }
            Kind::Uint64 => Ok(Some(Value::Uint(v))),
            Kind::Float64 => Ok(Some(Value::Float(v as f64))),
            _ => Err(self.mismatch("number")),
        }
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Option<Value>, E> {
        match self.kind {
            Kind::Float64 if !self.string => Ok(Some(Value::Float(v))),
            _ => Err(self.mismatch("number")),
        }
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Option<Value>, E> {
        match self.kind {
            Kind::String => Ok(Some(Value::Str(v.to_string()))),
            Kind::Int | Kind::Int64 | Kind::PtrInt64 | Kind::Uint64 if self.string => {
                self.number(v)
            }
            Kind::Float64 => Ok(Some(Value::Float(match v {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                _ if self.string => v.parse().map_err(|_| self.mismatch::<E>("string"))?,
                _ => return Err(self.mismatch("string")),
            }))),
            _ => Err(self.mismatch("string")),
        }
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Option<Value>, E> {
        match self.kind {
            Kind::String => Ok(Some(Value::Str(v))),
            _ => self.visit_str(&v),
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Option<Value>, A::Error> {
        match self.kind {
            Kind::Strings => {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(s) = seq.next_element_seed(StrSeed(&self))? {
                    out.push(s);
                }
                Ok(Some(Value::Strings(out)))
            }
            Kind::PtrSlice(t) => {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(item) = seq.next_element_seed(NullableStruct { ty: t })? {
                    out.push(item.map_or(Value::Nil, |s| Value::Struct(Box::new(s))));
                }
                Ok(Some(Value::List(out)))
            }
            _ => Err(self.mismatch("array")),
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Option<Value>, A::Error> {
        match self.kind {
            Kind::Ptr(t) | Kind::Inline(t) => Ok(Some(Value::Struct(Box::new(
                StructVisitor { ty: t }.visit_map(map)?,
            )))),
            Kind::Map(t) => {
                let mut out = Vec::new();
                while let Some(k) = map.next_key::<String>()? {
                    let s = map.next_value_seed(StructSeed { ty: t })?;
                    out.push((k, Value::Struct(Box::new(s))));
                }
                Ok(Some(Value::Map(out)))
            }
            Kind::Header => {
                let mut out = Vec::new();
                while let Some(k) = map.next_key::<String>()? {
                    let v: Vec<String> = map.next_value()?;
                    out.push((k, v));
                }
                Ok(Some(Value::Header(out)))
            }
            _ => Err(self.mismatch("object")),
        }
    }
}

struct StrSeed<'a>(&'a FieldSeed);

impl<'de> DeserializeSeed<'de> for StrSeed<'_> {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for StrSeed<'_> {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("string")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
        Ok(v.to_string())
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
        Ok(v)
    }

    fn visit_unit<E: de::Error>(self) -> Result<String, E> {
        Ok(String::new())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<String, E> {
        Err(self.0.mismatch("number"))
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<String, E> {
        Err(self.0.mismatch("number"))
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<String, E> {
        Err(self.0.mismatch("number"))
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<String, E> {
        Err(self.0.mismatch("bool"))
    }
}

struct NullableStruct {
    ty: usize,
}

impl<'de> DeserializeSeed<'de> for NullableStruct {
    type Value = Option<Struct>;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Option<Struct>, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for NullableStruct {
    type Value = Option<Struct>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "youtube.{}", type_def(self.ty).name)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Option<Struct>, E> {
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Option<Struct>, A::Error> {
        StructVisitor { ty: self.ty }.visit_map(map).map(Some)
    }
}
