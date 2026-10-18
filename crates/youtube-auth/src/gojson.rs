//! Go's `encoding/json/v2` notion of valid JSON (`utils.IsJson`): RFC 8259
//! with UTF-8 checked, lone surrogate escapes and duplicate object member
//! names rejected, and nesting capped at 10000.

use std::collections::HashSet;

const MAX_DEPTH: usize = 10000;

pub fn is_json(input: &[u8]) -> bool {
    std::str::from_utf8(input).is_ok() && Parser { s: input, i: 0 }.document().is_some()
}

enum Frame {
    Array,
    Object(HashSet<Vec<u8>>),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        (self.peek() == Some(c)).then(|| self.i += 1)
    }

    fn document(&mut self) -> Option<()> {
        let mut stack: Vec<Frame> = Vec::new();
        self.ws();
        // Each pass parses one value, then unwinds closed containers.
        loop {
            self.ws();
            match self.peek()? {
                b'[' => {
                    self.i += 1;
                    self.ws();
                    if self.eat(b']').is_none() {
                        stack.push(Frame::Array);
                        if stack.len() > MAX_DEPTH {
                            return None;
                        }
                        continue;
                    }
                }
                b'{' => {
                    self.i += 1;
                    self.ws();
                    if self.eat(b'}').is_none() {
                        let mut names = HashSet::new();
                        self.member_name(&mut names)?;
                        stack.push(Frame::Object(names));
                        if stack.len() > MAX_DEPTH {
                            return None;
                        }
                        continue;
                    }
                }
                b'"' => {
                    self.string()?;
                }
                b't' => self.literal(b"true")?,
                b'f' => self.literal(b"false")?,
                b'n' => self.literal(b"null")?,
                _ => self.number()?,
            }
            loop {
                self.ws();
                match stack.last_mut() {
                    None => {
                        return (self.i == self.s.len()).then_some(());
                    }
                    Some(Frame::Array) => match self.peek()? {
                        b',' => {
                            self.i += 1;
                            break;
                        }
                        b']' => {
                            self.i += 1;
                            stack.pop();
                        }
                        _ => return None,
                    },
                    Some(Frame::Object(names)) => match self.peek()? {
                        b',' => {
                            self.i += 1;
                            self.ws();
                            self.member_name(names)?;
                            break;
                        }
                        b'}' => {
                            self.i += 1;
                            stack.pop();
                        }
                        _ => return None,
                    },
                }
            }
        }
    }

    fn member_name(&mut self, names: &mut HashSet<Vec<u8>>) -> Option<()> {
        if self.peek()? != b'"' {
            return None;
        }
        let name = self.string()?;
        if !names.insert(name) {
            return None;
        }
        self.ws();
        self.eat(b':')
    }

    fn literal(&mut self, word: &[u8]) -> Option<()> {
        let end = self.i + word.len();
        (self.s.get(self.i..end)? == word).then(|| self.i = end)
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - start
    }

    fn number(&mut self) -> Option<()> {
        self.eat(b'-');
        match self.peek()? {
            b'0' => self.i += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        if self.eat(b'.').is_some() && self.digits() == 0 {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        Some(())
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.i += 4;
        u32::from_str_radix(h, 16).ok()
    }

    /// Parses a string and returns its unescaped bytes.
    fn string(&mut self) -> Option<Vec<u8>> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = self.peek()?;
            self.i += 1;
            match c {
                b'"' => return Some(out),
                0x00..=0x1f => return None,
                b'\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\x08',
                        b'f' => '\x0c',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = match hi {
                                0xd800..=0xdbff => {
                                    self.eat(b'\\')?;
                                    self.eat(b'u')?;
                                    let lo = self.hex4()?;
                                    if !(0xdc00..=0xdfff).contains(&lo) {
                                        return None;
                                    }
                                    0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00)
                                }
                                0xdc00..=0xdfff => return None,
                                c => c,
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_json;

    #[test]
    fn matches_go_jsontext() {
        // Each verdict was produced by Go 1.27.1 json.Unmarshal into jsontext.Value.
        for (input, want) in [
            (&b"{\"a\":1,\"a\":2}"[..], false),
            (b" 1 ", true),
            (b"\"x\"", true),
            (b"[1,]", false),
            (b"1 2", false),
            (b"\"\\ud800\"", false),
            (b"\"\\udc00\\ud800\"", false),
            (b"\"\\ud800\\udc00\"", true),
            (b"NaN", false),
            (b"-0", true),
            (b"1e999", true),
            (b"01", false),
            (b"", false),
            (b" ", false),
            (b"{\"a\":1}x", false),
            (b"nul", false),
            (b"\"a\\x\"", false),
            (b"\"\x01\"", false),
            (b"\"\x7f\"", true),
            (b"{\"a\":{\"b\":1},\"a2\":[1,{\"c\":1,\"c\":2}]}", false),
            (b"{\"a\":{\"b\":1},\"b\":{\"b\":1}}", true),
            (b"1.", false),
            (b".5", false),
            (b"-", false),
            (b"1E+2", true),
            (b"true", true),
            (b"{\"\\u0061\":1,\"a\":2}", false),
            (b"\t\r\n1\n", true),
            (b"{\"a\":1,}", false),
            (b"\xef\xbb\xbf1", false),
            (b"\"\xff\"", false),
            (b"\"\\/\"", true),
            (b"[]", true),
            (b"{}", true),
            (b"[[[]]]", true),
            (b"\x0b1", false),
            (b"\x0c1", false),
            (b"1\x00", false),
            (b"-01", false),
            (b"0.0e-0", true),
            ("\"\u{2028}\"".as_bytes(), true),
        ] {
            assert_eq!(is_json(input), want, "{:?}", String::from_utf8_lossy(input));
        }
    }
}
