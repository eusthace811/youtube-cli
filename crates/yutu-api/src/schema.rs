//! Shapes of the generated type and method tables.

/// A Go struct from google.golang.org/api/youtube/v3, with fields in Go order.
#[derive(Debug)]
pub struct TypeDef {
    pub name: &'static str,
    pub fields: &'static [Field],
}

#[derive(Debug)]
pub struct Field {
    /// Go field name, which yaml.v3 lowercases into the YAML key.
    pub go: &'static str,
    /// JSON key from the struct tag, empty when the tag is `json:"-"`.
    pub json: &'static str,
    pub kind: Kind,
    /// The `,string` tag option: the number travels as a JSON string.
    pub string: bool,
    pub omitempty: bool,
    pub skip: bool,
}

/// A Go field type. Indices name entries of [`crate::generated::TYPES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    String,
    Bool,
    Int,
    Int64,
    Uint64,
    Float64,
    PtrInt64,
    PtrBool,
    Strings,
    /// `map[string][]string`, the HTTP header in `googleapi.ServerResponse`.
    Header,
    /// `*T`
    Ptr(usize),
    /// An embedded struct value, `googleapi.ServerResponse`.
    Inline(usize),
    /// `[]*T`
    PtrSlice(usize),
    /// `map[string]T`
    Map(usize),
}

#[derive(Debug)]
pub struct Method {
    pub id: &'static str,
    pub http_method: &'static str,
    pub path: &'static str,
    pub upload_path: Option<&'static str>,
    pub request: Option<usize>,
    pub response: Option<usize>,
    pub download: bool,
    pub params: &'static [Param],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Query,
    Path,
}

#[derive(Debug)]
pub struct Param {
    pub name: &'static str,
    pub location: Location,
    pub repeated: bool,
    pub required: bool,
}

pub fn type_def(id: usize) -> &'static TypeDef {
    &crate::generated::TYPES[id]
}

pub fn method(id: &str) -> &'static Method {
    let methods = crate::generated::METHODS;
    let i = methods
        .binary_search_by(|m| m.id.cmp(id))
        .unwrap_or_else(|_| panic!("unknown API method {id}"));
    &methods[i]
}
