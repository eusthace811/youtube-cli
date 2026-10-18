//! `oauth2.Token`, encoded and decoded the way Go's `encoding/json/v2` does,
//! so token files move between the Go and Rust binaries unchanged.

use crate::gojson;
use chrono::{DateTime, FixedOffset, NaiveDate, TimeDelta, TimeZone};
use serde_json::Value;
use std::fmt;

/// oauth2's `defaultExpiryDelta`: a token counts as expired this long early.
const EXPIRY_DELTA: TimeDelta = TimeDelta::seconds(10);

#[derive(Clone, PartialEq)]
pub struct Token {
    pub access_token: String,
    pub token_type: String,
    pub refresh_token: String,
    pub expiry: GoTime,
    pub expires_in: i64,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Token")
            .field("access_token", &"[redacted]")
            .field("token_type", &self.token_type)
            .field("refresh_token", &"[redacted]")
            .field("expiry", &self.expiry.to_rfc3339_nano())
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

impl Token {
    pub fn empty() -> Token {
        Token {
            access_token: String::new(),
            token_type: String::new(),
            refresh_token: String::new(),
            expiry: GoTime::ZERO,
            expires_in: 0,
        }
    }

    /// `Token.Valid` at `now`.
    pub fn valid_at(&self, now: DateTime<FixedOffset>) -> bool {
        !self.access_token.is_empty() && !self.expiry.expired_at(now)
    }

    pub fn valid(&self) -> bool {
        self.valid_at(chrono::Local::now().fixed_offset())
    }

    /// The bytes `json.MarshalWrite(f, token)` writes.
    pub fn to_go_json(&self) -> Vec<u8> {
        let q = |s: &str| serde_json::to_string(s).expect("strings always encode");
        let mut out = String::from("{\"access_token\":");
        out.push_str(&q(&self.access_token));
        if !self.token_type.is_empty() {
            out.push_str(",\"token_type\":");
            out.push_str(&q(&self.token_type));
        }
        if !self.refresh_token.is_empty() {
            out.push_str(",\"refresh_token\":");
            out.push_str(&q(&self.refresh_token));
        }
        out.push_str(",\"expiry\":\"");
        out.push_str(&self.expiry.to_rfc3339_nano());
        out.push_str("\",\"expires_in\":");
        out.push_str(&self.expires_in.to_string());
        out.push('}');
        out.into_bytes()
    }

    /// `json.Unmarshal(data, &token)` with Go's v2 rules.
    pub fn from_go_json(data: &[u8]) -> Result<Token, String> {
        if !gojson::is_json(data) {
            return Err("invalid JSON".to_string());
        }
        let value: Value = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        let mut token = Token::empty();
        let fields = match value {
            Value::Null => return Ok(token),
            Value::Object(fields) => fields,
            other => return Err(format!("cannot unmarshal JSON {} into oauth2.Token", kind(&other))),
        };
        for (name, v) in fields {
            match name.as_str() {
                "access_token" => token.access_token = string_field(&name, v)?,
                "token_type" => token.token_type = string_field(&name, v)?,
                "refresh_token" => token.refresh_token = string_field(&name, v)?,
                "expiry" => {
                    token.expiry = match v {
                        Value::Null => GoTime::ZERO,
                        Value::String(s) => GoTime::parse_rfc3339(&s)
                            .ok_or_else(|| format!("cannot parse {s:?} as RFC 3339"))?,
                        other => return Err(mismatch(&name, &other)),
                    }
                }
                "expires_in" => {
                    token.expires_in = match v {
                        Value::Null => 0,
                        Value::Number(n) => n.as_i64().ok_or_else(|| mismatch(&name, &Value::Number(n)))?,
                        other => return Err(mismatch(&name, &other)),
                    }
                }
                _ => {}
            }
        }
        Ok(token)
    }
}

fn string_field(name: &str, v: Value) -> Result<String, String> {
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s),
        other => Err(mismatch(name, &other)),
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn mismatch(name: &str, v: &Value) -> String {
    format!("cannot unmarshal JSON {} into field {name}", kind(v))
}

/// A Go `time.Time` with the offset it was created or parsed in, so it
/// re-encodes byte for byte. Go's zero time means "never expires".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoTime(DateTime<FixedOffset>);

impl GoTime {
    pub const ZERO: GoTime = GoTime(DateTime::<FixedOffset>::from_naive_utc_and_offset(
        NaiveDate::from_ymd_opt(1, 1, 1)
            .expect("valid date")
            .and_hms_opt(0, 0, 0)
            .expect("valid time"),
        FixedOffset::east_opt(0).expect("valid offset"),
    ));

    pub fn new(t: DateTime<FixedOffset>) -> GoTime {
        GoTime(t)
    }

    /// `time.Now().Add(seconds)` in the local zone, as oauth2 sets `Expiry`.
    pub fn now_plus(seconds: i64) -> GoTime {
        GoTime(chrono::Local::now().fixed_offset() + TimeDelta::seconds(seconds))
    }

    pub fn is_zero(&self) -> bool {
        self.0 == GoTime::ZERO.0
    }

    fn expired_at(&self, now: DateTime<FixedOffset>) -> bool {
        !self.is_zero() && self.0 - EXPIRY_DELTA < now
    }

    pub fn datetime(&self) -> DateTime<FixedOffset> {
        self.0
    }

    /// Go's `time.RFC3339Nano` layout.
    pub fn to_rfc3339_nano(&self) -> String {
        let mut out = self.0.format("%Y-%m-%dT%H:%M:%S").to_string();
        let nanos = self.0.timestamp_subsec_nanos();
        if nanos != 0 {
            let frac = format!("{nanos:09}");
            out.push('.');
            out.push_str(frac.trim_end_matches('0'));
        }
        crate::log::push_offset(&mut out, self.0.offset().local_minus_utc());
        out
    }

    /// Strict RFC 3339 as Go's JSON decoder accepts it for `time.Time`.
    pub fn parse_rfc3339(s: &str) -> Option<GoTime> {
        let b = s.as_bytes();
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            let d = b.get(r)?;
            d.iter().all(u8::is_ascii_digit).then(|| {
                d.iter().fold(0u32, |acc, c| acc * 10 + u32::from(c - b'0'))
            })
        };
        let sep = |i: usize, c: u8| b.get(i) == Some(&c);
        if !(sep(4, b'-') && sep(7, b'-') && sep(10, b'T') && sep(13, b':') && sep(16, b':')) {
            return None;
        }
        let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
        let (hour, min, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
        if hour > 23 || min > 59 || sec > 59 {
            return None;
        }
        let mut i = 19;
        let mut nanos = 0u32;
        if sep(i, b'.') {
            let start = i + 1;
            let mut end = start;
            while b.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            let digits = end - start;
            if digits == 0 || digits > 9 {
                return None;
            }
            nanos = num(start..end)? * 10u32.pow(9 - digits as u32);
            i = end;
        }
        let offset = match b.get(i)? {
            b'Z' if i + 1 == b.len() => 0,
            &c @ (b'+' | b'-') if i + 6 == b.len() && sep(i + 3, b':') => {
                let (h, m) = (num(i + 1..i + 3)?, num(i + 4..i + 6)?);
                if h > 23 || m > 59 {
                    return None;
                }
                let secs = (h * 3600 + m * 60) as i32;
                if c == b'-' { -secs } else { secs }
            }
            _ => return None,
        };
        let date = NaiveDate::from_ymd_opt(year as i32, month, day)?;
        let naive = date.and_hms_nano_opt(hour, min, sec, nanos)?;
        let t = FixedOffset::east_opt(offset)?
            .from_local_datetime(&naive)
            .single()?;
        Some(GoTime(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<FixedOffset> {
        GoTime::parse_rfc3339(s).unwrap().0
    }

    #[test]
    fn encodes_like_go_marshal_write() {
        // Expected bytes captured from Go 1.27.1 json.MarshalWrite on oauth2.Token.
        let full = Token {
            access_token: "ya29.a0AfH6SM<&>\u{2028}x".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: "1//0gRefresh".to_string(),
            expiry: GoTime(at("2026-10-18T16:55:30.123456+01:00")),
            expires_in: 3599,
        };
        assert_eq!(
            String::from_utf8(full.to_go_json()).unwrap(),
            "{\"access_token\":\"ya29.a0AfH6SM<&>\u{2028}x\",\"token_type\":\"Bearer\",\"refresh_token\":\"1//0gRefresh\",\"expiry\":\"2026-10-18T16:55:30.123456+01:00\",\"expires_in\":3599}"
        );
        let utc = Token {
            access_token: "plain".to_string(),
            expiry: GoTime(at("2026-01-02T03:04:05Z")),
            ..Token::empty()
        };
        assert_eq!(
            String::from_utf8(utc.to_go_json()).unwrap(),
            "{\"access_token\":\"plain\",\"expiry\":\"2026-01-02T03:04:05Z\",\"expires_in\":0}"
        );
        let ctl = Token {
            access_token: "ctl\x01\t\"\\\x7f\x08\x0c".to_string(),
            token_type: "Bearer".to_string(),
            ..Token::empty()
        };
        assert_eq!(
            String::from_utf8(ctl.to_go_json()).unwrap(),
            "{\"access_token\":\"ctl\\u0001\\t\\\"\\\\\x7f\\b\\f\",\"token_type\":\"Bearer\",\"expiry\":\"0001-01-01T00:00:00Z\",\"expires_in\":0}"
        );
        assert_eq!(
            String::from_utf8(Token::empty().to_go_json()).unwrap(),
            "{\"access_token\":\"\",\"expiry\":\"0001-01-01T00:00:00Z\",\"expires_in\":0}"
        );
    }

    #[test]
    fn decodes_and_reencodes_unchanged() {
        let bytes = b"{\"access_token\":\"a\",\"token_type\":\"Bearer\",\"refresh_token\":\"r\",\"expiry\":\"2026-10-18T16:55:30.1+01:00\",\"expires_in\":3599}";
        let token = Token::from_go_json(bytes).unwrap();
        assert_eq!(token.to_go_json(), bytes.to_vec());
    }

    #[test]
    fn decode_rejects_what_go_rejects() {
        assert!(Token::from_go_json(b"{\"access_token\":\"a\",\"access_token\":\"b\"}").is_err());
        assert!(Token::from_go_json(b"{\"access_token\":1}").is_err());
        assert!(Token::from_go_json(b"[]").is_err());
        assert!(Token::from_go_json(b"{\"expiry\":\"2026-10-18 16:55:30Z\"}").is_err());
        assert!(Token::from_go_json(b"{\"expires_in\":1.5}").is_err());
        assert_eq!(Token::from_go_json(b"null").unwrap(), Token::empty());
        assert_eq!(
            Token::from_go_json(b"{\"Access_Token\":\"x\"}").unwrap(),
            Token::empty()
        );
    }

    #[test]
    fn validity_uses_ten_second_delta() {
        let now = at("2026-01-01T00:00:00Z");
        let token = |expiry: &str| Token {
            access_token: "a".to_string(),
            expiry: GoTime(at(expiry)),
            ..Token::empty()
        };
        assert!(token("2026-01-01T00:00:11Z").valid_at(now));
        assert!(token("2026-01-01T00:00:10Z").valid_at(now));
        assert!(!token("2026-01-01T00:00:09Z").valid_at(now));
        assert!(token("0001-01-01T01:00:00+01:00").valid_at(now));
        assert!(!Token::empty().valid_at(now));
    }
}
