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

impl ApiError {
    /// googleapi.CheckResponseWithBody: parse `{"error": {...}}` when present.
    pub fn from_body(status: u16, body: &[u8]) -> ApiError {
        let text = String::from_utf8_lossy(body).into_owned();
        let parsed: Option<Json> = serde_json::from_slice(body).ok();
        let err = parsed
            .as_ref()
            .and_then(|j| j.get("error"))
            .filter(|e| e.is_object());
        let Some(e) = err else {
            return ApiError {
                code: status,
                message: String::new(),
                details: Vec::new(),
                errors: Vec::new(),
                body: text,
            };
        };
        let code = e
            .get("code")
            .and_then(Json::as_u64)
            .filter(|c| *c != 0)
            .map(|c| c as u16)
            .unwrap_or(status);
        let s = |v: Option<&Json>| v.and_then(Json::as_str).unwrap_or("").to_string();
        ApiError {
            code,
            message: s(e.get("message")),
            details: e
                .get("details")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default(),
            errors: e
                .get("errors")
                .and_then(Json::as_array)
                .map(|a| {
                    a.iter()
                        .map(|i| ErrorItem {
                            reason: s(i.get("reason")),
                            message: s(i.get("message")),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            body: text,
        }
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
            let mut pretty = serde_json::to_string_pretty(&self.details).unwrap_or_default();
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
    fn formats_multiple_errors() {
        let body = br#"{"error":{"code":400,"message":"Bad","errors":[{"message":"a","reason":"r1"},{"message":"b","reason":"r2"}]}}"#;
        assert_eq!(
            ApiError::from_body(400, body).to_string(),
            "googleapi: Error 400: Bad\nMore details:\nReason: r1, Message: a\nReason: r2, Message: b\n"
        );
    }
}
