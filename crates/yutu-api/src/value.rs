//! A typed value tree mirroring a Go struct from the YouTube client, so the
//! codecs can reproduce Go's zero values, nil pointers and field order.

use crate::schema::{Kind, type_def};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A nil pointer, slice or map.
    Nil,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(String),
    Strings(Vec<String>),
    /// `map[string][]string`, kept in insertion order and sorted on output.
    Header(Vec<(String, Vec<String>)>),
    Struct(Box<Struct>),
    /// `[]*T`; elements are `Struct` or `Nil`.
    List(Vec<Value>),
    /// `map[string]T`; values are `Struct`.
    Map(Vec<(String, Value)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub ty: usize,
    pub fields: Vec<Value>,
}

impl Value {
    pub fn zero(kind: Kind) -> Value {
        match kind {
            Kind::String => Value::Str(String::new()),
            Kind::Bool => Value::Bool(false),
            Kind::Int | Kind::Int64 => Value::Int(0),
            Kind::Uint64 => Value::Uint(0),
            Kind::Float64 => Value::Float(0.0),
            Kind::Inline(t) => Value::Struct(Box::new(Struct::new(t))),
            Kind::PtrInt64
            | Kind::PtrBool
            | Kind::Strings
            | Kind::Header
            | Kind::Ptr(_)
            | Kind::PtrSlice(_)
            | Kind::Map(_) => Value::Nil,
        }
    }

    pub fn new_struct(ty: usize) -> Value {
        Value::Struct(Box::new(Struct::new(ty)))
    }

    pub fn as_struct(&self) -> Option<&Struct> {
        match self {
            Value::Struct(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_struct_mut(&mut self) -> Option<&mut Struct> {
        match self {
            Value::Struct(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Value::Str(s) => s,
            _ => "",
        }
    }

    pub fn as_list(&self) -> &[Value] {
        match self {
            Value::List(v) => v,
            _ => &[],
        }
    }

    pub fn as_strings(&self) -> &[String] {
        match self {
            Value::Strings(v) => v,
            _ => &[],
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Follows a dotted path of JSON field names through non-nil pointers.
    pub fn at(&self, path: &str) -> Option<&Value> {
        let mut cur = self;
        for key in path.split('.') {
            cur = cur.as_struct()?.get(key)?;
            if cur.is_nil() {
                return None;
            }
        }
        Some(cur)
    }

    /// The string at `path`, or "" when any pointer on the way is nil.
    pub fn str(&self, path: &str) -> &str {
        self.at(path).map(Value::as_str).unwrap_or("")
    }

    /// Mutable access to the struct at `path`, allocating nil pointers like
    /// `x.Snippet = &youtube.VideoSnippet{}` would.
    pub fn ensure(&mut self, path: &str) -> &mut Struct {
        let mut cur = self.as_struct_mut().expect("ensure on a non-struct value");
        if path.is_empty() {
            return cur;
        }
        for key in path.split('.') {
            let i = cur
                .index(key)
                .unwrap_or_else(|| panic!("no field {key} in {}", type_def(cur.ty).name));
            let ty = match type_def(cur.ty).fields[i].kind {
                Kind::Ptr(t) | Kind::Inline(t) => t,
                k => panic!("field {key} is {k:?}, not a struct"),
            };
            if cur.fields[i].is_nil() {
                cur.fields[i] = Value::new_struct(ty);
            }
            cur = cur.fields[i].as_struct_mut().unwrap();
        }
        cur
    }
}

impl Struct {
    pub fn new(ty: usize) -> Struct {
        Struct {
            ty,
            fields: type_def(ty)
                .fields
                .iter()
                .map(|f| Value::zero(f.kind))
                .collect(),
        }
    }

    pub fn index(&self, json: &str) -> Option<usize> {
        type_def(self.ty)
            .fields
            .iter()
            .position(|f| f.json == json && !f.skip)
    }

    pub fn get(&self, json: &str) -> Option<&Value> {
        self.index(json).map(|i| &self.fields[i])
    }

    /// Sets the field with JSON name `json`. Panics on an unknown name, which
    /// is a programming error in an op.
    pub fn set(&mut self, json: &str, value: Value) -> &mut Self {
        let i = self
            .index(json)
            .unwrap_or_else(|| panic!("no field {json} in {}", type_def(self.ty).name));
        self.fields[i] = value;
        self
    }

    pub fn set_str(&mut self, json: &str, value: impl Into<String>) -> &mut Self {
        self.set(json, Value::Str(value.into()))
    }

    /// Adds a Go field name to ForceSendFields, as Go code does to send a
    /// zero value such as `false` in an update.
    pub fn force_send(&mut self, go_field: &str) -> &mut Self {
        let i = type_def(self.ty)
            .fields
            .iter()
            .position(|f| f.go == "ForceSendFields")
            .expect("ForceSendFields");
        match &mut self.fields[i] {
            Value::Strings(v) => v.push(go_field.to_string()),
            other => *other = Value::Strings(vec![go_field.to_string()]),
        }
        self
    }

    pub fn go_strings(&self, go_field: &str) -> &[String] {
        type_def(self.ty)
            .fields
            .iter()
            .position(|f| f.go == go_field)
            .map(|i| self.fields[i].as_strings())
            .unwrap_or(&[])
    }
}
