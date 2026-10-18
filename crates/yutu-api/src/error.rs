//! API errors formatted like google.golang.org/api/googleapi.Error.

use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A non-2xx response from the API.
    Api(ApiError),
    /// The request never got a response: DNS, TCP, TLS, timeout.
    Transport(String),
    /// The response body did not decode into the expected Go type.
    Decode(String),
    /// Local failures before sending, such as an unreadable media file.
    Local(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub code: u16,
    pub message: String,
    pub details: Vec<Json>,
    pub errors: Vec<ErrorItem>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorItem {
    pub reason: String,
    pub message: String,
}

#[derive(serde::Deserialize)]
struct Reply {
    error: Option<ErrorBody>,
}

#[derive(serde::Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    details: Vec<Json>,
    #[serde(default)]
    errors: Vec<Item>,
}

#[derive(serde::Deserialize)]
struct Item {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    message: String,
}

impl ApiError {
    /// googleapi.CheckResponseWithBody: a body holding `{"error": {...}}`, or
    /// an array whose first element does, becomes the structured error; any
    /// other body, including one with mistyped fields, is kept raw.
    pub fn from_body(status: u16, body: &[u8]) -> ApiError {
        let text = String::from_utf8_lossy(body).into_owned();
        let parsed = if body.first() == Some(&b'[') {
            serde_json::from_slice::<Vec<Reply>>(body)
                .ok()
                .and_then(|v| v.into_iter().next())
        } else {
            serde_json::from_slice::<Reply>(body).ok()
        };
        let Some(e) = parsed.and_then(|r| r.error) else {
            return ApiError {
                code: status,
                message: String::new(),
                details: Vec::new(),
                errors: Vec::new(),
                body: text,
            };
        };
        ApiError {
            code: if e.code == 0 { status } else { e.code as u16 },
            message: e.message,
            details: e.details,
            errors: e
                .errors
                .into_iter()
                .map(|i| ErrorItem {
                    reason: i.reason,
                    message: i.message,
                })
                .collect(),
            body: text,
        }
    }
}

/// Go marshals decoded `interface{}` maps with sorted keys.
fn sorted(v: &Json) -> Json {
    match v {
        Json::Object(m) => {
            let mut keys: Vec<_> = m.keys().collect();
            keys.sort();
            Json::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), sorted(&m[k])))
                    .collect(),
            )
        }
        Json::Array(a) => Json::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.errors.is_empty() && self.message.is_empty() {
            return write!(
                f,
                "googleapi: got HTTP response code {} with body: {}",
                self.code, self.body
            );
        }
        let mut buf = format!("googleapi: Error {}: {}", self.code, self.message);
        if !self.details.is_empty() {
            let details: Vec<Json> = self.details.iter().map(sorted).collect();
            let mut pretty = serde_json::to_string_pretty(&details).unwrap_or_default();
            pretty = pretty
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
                .replace('&', "\\u0026");
            buf.push_str("\nDetails:\n");
            buf.push_str(&pretty);
            buf.push('\n');
        }
        if self.errors.is_empty() {
            return f.write_str(buf.trim());
        }
        if self.errors.len() == 1 && self.errors[0].message == self.message {
            return write!(f, "{buf}, {}", self.errors[0].reason);
        }
        buf.push_str("\nMore details:\n");
        for e in &self.errors {
            buf.push_str(&format!("Reason: {}, Message: {}\n", e.reason, e.message));
        }
        f.write_str(&buf)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(e) => e.fmt(f),
            Error::Transport(s) | Error::Decode(s) | Error::Local(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_single_reason_like_go() {
        let body = br#"{"error":{"code":403,"message":"The request is not allowed.","errors":[{"message":"The request is not allowed.","domain":"global","reason":"forbidden"}]}}"#;
        assert_eq!(
            ApiError::from_body(403, body).to_string(),
            "googleapi: Error 403: The request is not allowed., forbidden"
        );
    }

    #[test]
    fn formats_bodies_without_error_object() {
        assert_eq!(
            ApiError::from_body(502, b"bad gateway").to_string(),
            "googleapi: got HTTP response code 502 with body: bad gateway"
        );
    }

    #[test]
    fn sorts_detail_keys_like_go() {
        let body = br#"{"error":{"code":403,"message":"No","details":[{"@type":"t","reason":"R","domain":"d"}]}}"#;
        assert_eq!(
            ApiError::from_body(403, body).to_string(),
            "googleapi: Error 403: No\nDetails:\n[\n  {\n    \"@type\": \"t\",\n    \"domain\": \"d\",\n    \"reason\": \"R\"\n  }\n]"
        );
    }

    #[test]
    fn reads_array_bodies_and_rejects_mistyped_fields() {
        let array = br#"[{"error":{"code":404,"message":"gone","errors":[{"message":"gone","reason":"notFound"}]}}]"#;
        assert_eq!(
            ApiError::from_body(404, array).to_string(),
            "googleapi: Error 404: gone, notFound"
        );
        let mistyped = br#"{"error":{"code":1.5,"message":"x"}}"#;
        assert_eq!(
            ApiError::from_body(500, mistyped).to_string(),
            "googleapi: got HTTP response code 500 with body: {\"error\":{\"code\":1.5,\"message\":\"x\"}}"
        );
    }

    #[test]
    fn formats_multiple_errors() {
        let body = br#"{"error":{"code":400,"message":"Bad","errors":[{"message":"a","reason":"r1"},{"message":"b","reason":"r2"}]}}"#;
        assert_eq!(
            ApiError::from_body(400, body).to_string(),
            "googleapi: Error 400: Bad\nMore details:\nReason: r1, Message: a\nReason: r2, Message: b\n"
        );
    }
}
