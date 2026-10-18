//! Go wire details the launchers reproduce: random identifiers, RFC 3339
//! timestamps and `encoding/json`'s decode error texts.

use serde_json::Value;

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("OS randomness");
    b
}

fn uuid_string(b: [u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// A random (version 4) UUID, as ADK names events and sessions.
pub fn uuid4() -> String {
    let mut b = random::<16>();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid_string(b)
}

/// A time-ordered (version 7) UUID, as a2a-go names tasks and contexts.
pub fn uuid7() -> String {
    let mut b = random::<16>();
    let ms = chrono::Utc::now().timestamp_millis() as u64;
    b[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    uuid_string(b)
}

/// A per-launch bearer token: 32 random bytes, base64url without padding.
pub fn token() -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = random::<32>();
    let mut out = String::with_capacity(43);
    let mut acc = 0u32;
    let mut bits = 0;
    for b in bytes {
        acc = (acc << 8) | u32::from(b);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(ALPHABET[((acc >> bits) & 63) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (6 - bits)) & 63) as usize] as char);
    }
    out
}

/// Go's `time.Now()` marshalled to JSON: RFC 3339 with the local offset and
/// trailing zeros of the fraction dropped.
pub fn now_rfc3339() -> String {
    let now = chrono::Local::now().fixed_offset();
    let base = now.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = format!("{:09}", now.timestamp_subsec_nanos());
    let frac = nanos.trim_end_matches('0');
    let offset = now.format("%:z").to_string();
    let offset = if offset == "+00:00" {
        "Z".into()
    } else {
        offset
    };
    if frac.is_empty() {
        format!("{base}{offset}")
    } else {
        format!("{base}.{frac}{offset}")
    }
}

/// Seconds since the epoch.
pub fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Decodes a JSON body the way Go's `json.Decoder.Decode` reports failures:
/// `EOF` for an empty body, `unexpected EOF` for a truncated one, and the
/// scanner's `invalid character ...` texts.
pub fn decode(body: &[u8]) -> Result<Value, String> {
    let text =
        std::str::from_utf8(body).map_err(|_| "invalid character in string literal".to_string())?;
    if text.trim_matches([' ', '\t', '\n', '\r']).is_empty() {
        return Err("EOF".into());
    }
    let mut s = Scanner {
        b: text.as_bytes(),
        i: 0,
    };
    s.ws();
    s.value()?;
    // Decode reads one value; whatever follows is left for the next call.
    serde_json::from_str::<Value>(&text[..s.i]).map_err(|e| e.to_string())
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
}

fn quote_char(c: u8) -> String {
    match c {
        b'\'' => "'\\''".into(),
        b'"' => "'\"'".into(),
        c if c.is_ascii_graphic() || c == b' ' => format!("'{}'", c as char),
        c => yutu_auth::log::quote(&(c as char).to_string()),
    }
}

impl Scanner<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn next(&mut self) -> Result<u8, String> {
        let c = *self.b.get(self.i).ok_or("unexpected EOF")?;
        self.i += 1;
        Ok(c)
    }

    fn invalid(c: u8, context: &str) -> String {
        format!("invalid character {} {context}", quote_char(c))
    }

    fn value(&mut self) -> Result<(), String> {
        let c = self.next()?;
        match c {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string(),
            b't' => self.literal(b"rue", "true"),
            b'f' => self.literal(b"alse", "false"),
            b'n' => self.literal(b"ull", "null"),
            b'-' | b'0'..=b'9' => self.number(c),
            c => Err(Self::invalid(c, "looking for beginning of value")),
        }
    }

    fn literal(&mut self, rest: &[u8], name: &str) -> Result<(), String> {
        for &want in rest {
            let c = self.next()?;
            if c != want {
                return Err(format!(
                    "invalid character {} in literal {name} (expecting {})",
                    quote_char(c),
                    quote_char(want)
                ));
            }
        }
        Ok(())
    }

    fn number(&mut self, first: u8) -> Result<(), String> {
        let mut c = first;
        if c == b'-' {
            c = self.next()?;
            if !c.is_ascii_digit() {
                return Err(Self::invalid(c, "in numeric literal"));
            }
        }
        if c != b'0' {
            while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            let c = self.next()?;
            if !c.is_ascii_digit() {
                return Err(Self::invalid(c, "after decimal point in numeric literal"));
            }
            while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            let mut c = self.next()?;
            if c == b'+' || c == b'-' {
                c = self.next()?;
            }
            if !c.is_ascii_digit() {
                return Err(Self::invalid(c, "in exponent of numeric literal"));
            }
            while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        Ok(())
    }

    fn string(&mut self) -> Result<(), String> {
        loop {
            let c = self.next()?;
            match c {
                b'"' => return Ok(()),
                b'\\' => {
                    let e = self.next()?;
                    match e {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            for _ in 0..4 {
                                let h = self.next()?;
                                if !h.is_ascii_hexdigit() {
                                    return Err(Self::invalid(
                                        h,
                                        "in \\u hexadecimal character escape",
                                    ));
                                }
                            }
                        }
                        e => return Err(Self::invalid(e, "in string escape code")),
                    }
                }
                c if c < 0x20 => return Err(Self::invalid(c, "in string literal")),
                _ => {}
            }
        }
    }

    fn object(&mut self) -> Result<(), String> {
        self.ws();
        let mut c = self.next()?;
        if c == b'}' {
            return Ok(());
        }
        loop {
            if c != b'"' {
                return Err(Self::invalid(
                    c,
                    "looking for beginning of object key string",
                ));
            }
            self.string()?;
            self.ws();
            let colon = self.next()?;
            if colon != b':' {
                return Err(Self::invalid(colon, "after object key"));
            }
            self.ws();
            self.value()?;
            self.ws();
            let sep = self.next()?;
            match sep {
                b',' => {
                    self.ws();
                    c = self.next()?;
                }
                b'}' => return Ok(()),
                s => return Err(Self::invalid(s, "after object key:value pair")),
            }
        }
    }

    fn array(&mut self) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(());
        }
        loop {
            self.ws();
            self.value()?;
            self.ws();
            match self.next()? {
                b',' => {}
                b']' => return Ok(()),
                s => return Err(Self::invalid(s, "after array element")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_errors_read_like_go() {
        assert_eq!(decode(b"").unwrap_err(), "EOF");
        assert_eq!(decode(b"{").unwrap_err(), "unexpected EOF");
        assert_eq!(
            decode(b"not json").unwrap_err(),
            "invalid character 'o' in literal null (expecting 'u')"
        );
        assert_eq!(
            decode(b"x").unwrap_err(),
            "invalid character 'x' looking for beginning of value"
        );
        assert_eq!(
            decode(b"{\"a\" 1}").unwrap_err(),
            "invalid character '1' after object key"
        );
        assert_eq!(
            decode(b"{\"a\":1 \"b\"}").unwrap_err(),
            "invalid character '\"' after object key:value pair"
        );
        assert_eq!(
            decode(b"{\"a\":[1,2]} trailing").unwrap(),
            serde_json::json!({"a": [1, 2]})
        );
    }

    #[test]
    fn identifiers_have_their_versions() {
        let v4 = uuid4();
        assert_eq!(v4.len(), 36);
        assert_eq!(&v4[14..15], "4");
        assert_eq!(&uuid7()[14..15], "7");
        assert_eq!(token().len(), 43);
        assert_ne!(token(), token());
    }
}
