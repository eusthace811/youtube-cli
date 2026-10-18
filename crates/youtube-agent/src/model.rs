//! The conversation as both providers see it: genai's `Content` shape, which
//! ADK keeps in the session and each client translates on the wire.

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Model,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall {
    /// The provider's call id; Gemini usually sends none.
    pub id: Option<String>,
    pub name: String,
    pub args: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionResponse {
    pub id: Option<String>,
    pub name: String,
    /// `{"output": ...}` on success, `{"error": ...}` on failure, as ADK
    /// builds it.
    pub response: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text {
        text: String,
        thought: bool,
        /// Gemini's opaque `thoughtSignature`, echoed back unchanged.
        signature: Option<String>,
    },
    Call {
        call: FunctionCall,
        signature: Option<String>,
    },
    Response(FunctionResponse),
}

impl Part {
    pub fn text(text: impl Into<String>) -> Part {
        Part::Text {
            text: text.into(),
            thought: false,
            signature: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Content {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Content {
    pub fn user_text(text: impl Into<String>) -> Content {
        Content {
            role: Role::User,
            parts: vec![Part::text(text)],
        }
    }

    pub fn calls(&self) -> impl Iterator<Item = &FunctionCall> {
        self.parts.iter().filter_map(|p| match p {
            Part::Call { call, .. } => Some(call),
            _ => None,
        })
    }

    /// Every text part joined, thoughts included, as ADK's console prints it.
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// A turn that only thought: ADK calls the model again instead of
    /// treating it as the answer.
    pub fn thought_only(&self) -> bool {
        !self.parts.is_empty()
            && self
                .parts
                .iter()
                .all(|p| matches!(p, Part::Text { thought: true, .. }))
    }
}

/// ADK's streaming aggregator: consecutive text of the same kind becomes one
/// part; calls keep their order.
pub fn merge(parts: &mut Vec<Part>, part: Part) {
    if let (
        Some(Part::Text {
            text: acc,
            thought: t0,
            signature: s0,
        }),
        Part::Text {
            text,
            thought,
            signature,
        },
    ) = (parts.last_mut(), &part)
        && *t0 == *thought
    {
        acc.push_str(text);
        if signature.is_some() {
            s0.clone_from(signature);
        }
        return;
    }
    parts.push(part);
}

/// One function the model may call, in Gemini's declaration shape.
#[derive(Debug, Clone)]
pub struct Declaration {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// Only ADK's function tools declare a response schema.
    pub response: Option<Value>,
}

/// What a client sends for one model call.
pub struct Request<'a> {
    pub system: &'a str,
    pub contents: &'a [Content],
    pub declarations: &'a [Declaration],
    pub google_search: bool,
}
