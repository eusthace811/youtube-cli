//! Go's `encoding/base64`. Decoding is hand-ported because the error offset
//! reaches users: a missing `client_secret.json` fails with
//! `illegal base64 data at input byte 6`.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn std_encode(src: &[u8]) -> String {
    encode(STD, true, src)
}

/// `base64.URLEncoding`, padded.
pub fn url_encode(src: &[u8]) -> String {
    encode(URL, true, src)
}

/// `base64.RawURLEncoding`.
pub fn raw_url_encode(src: &[u8]) -> String {
    encode(URL, false, src)
}

fn encode(alphabet: &[u8; 64], pad: bool, src: &[u8]) -> String {
    let mut out = String::with_capacity(src.len().div_ceil(3) * 4);
    for chunk in src.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..=chunk.len() {
            out.push(alphabet[(v >> (18 - 6 * i) & 0x3f) as usize] as char);
        }
        if pad {
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
    }
    out
}

/// `base64.CorruptInputError`.
#[derive(Debug, PartialEq, Eq)]
pub struct CorruptInput(pub usize);

impl std::fmt::Display for CorruptInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal base64 data at input byte {}", self.0)
    }
}

fn std_value(c: u8) -> Option<u8> {
    STD.iter().position(|&a| a == c).map(|p| p as u8)
}

/// `base64.StdEncoding.DecodeString`: padded, lenient trailing bits, and
/// `\r`/`\n` skipped anywhere.
pub fn std_decode(src: &[u8]) -> Result<Vec<u8>, CorruptInput> {
    let mut out = Vec::with_capacity(src.len() / 4 * 3);
    let mut si = 0;
    while si < src.len() {
        si = decode_quantum(&mut out, src, si)?;
    }
    Ok(out)
}

/// One step of Go's `decodeQuantum`; returns the next input offset.
fn decode_quantum(out: &mut Vec<u8>, src: &[u8], mut si: usize) -> Result<usize, CorruptInput> {
    let mut dbuf = [0u8; 4];
    let mut dlen = 4;
    let mut j = 0;
    while j < 4 {
        if si == src.len() {
            if j == 0 {
                return Ok(si);
            }
            // StdEncoding is padded, so a short final quantum is corrupt.
            return Err(CorruptInput(si - j));
        }
        let c = src[si];
        si += 1;
        if let Some(v) = std_value(c) {
            dbuf[j] = v;
            j += 1;
            continue;
        }
        if c == b'\n' || c == b'\r' {
            continue;
        }
        if c != b'=' {
            return Err(CorruptInput(si - 1));
        }
        match j {
            0 | 1 => return Err(CorruptInput(si - 1)),
            2 => {
                while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                    si += 1;
                }
                if si == src.len() {
                    return Err(CorruptInput(src.len()));
                }
                if src[si] != b'=' {
                    return Err(CorruptInput(si - 1));
                }
                si += 1;
            }
            _ => {}
        }
        while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
            si += 1;
        }
        if si < src.len() {
            return Err(CorruptInput(si));
        }
        dlen = j;
        break;
    }
    let val = u32::from(dbuf[0]) << 18
        | u32::from(dbuf[1]) << 12
        | u32::from(dbuf[2]) << 6
        | u32::from(dbuf[3]);
    let bytes = [(val >> 16) as u8, (val >> 8) as u8, val as u8];
    out.extend_from_slice(&bytes[..dlen - 1]);
    Ok(si)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_matches_go() {
        // Results from Go 1.27.1 base64.StdEncoding.DecodeString.
        assert_eq!(std_decode(b"client_secret.json"), Err(CorruptInput(6)));
        assert_eq!(std_decode(b"youtube.token.json"), Err(CorruptInput(7)));
        assert_eq!(std_decode(b"eyJhIjoxfQ=="), Ok(b"{\"a\":1}".to_vec()));
        assert_eq!(std_decode(b"eyJh\nIjox\r\nfQ=="), Ok(b"{\"a\":1}".to_vec()));
        assert_eq!(std_decode(b"eyJhIjoxfQ"), Err(CorruptInput(8)));
        assert_eq!(std_decode(b"eyJhIjoxfQ="), Err(CorruptInput(11)));
        assert_eq!(std_decode(b"eyJhIjoxfQ==x"), Err(CorruptInput(12)));
        assert_eq!(std_decode(b"YQ-_"), Err(CorruptInput(2)));
        assert_eq!(std_decode(b"YR=="), Ok(b"a".to_vec()));
        assert_eq!(std_decode(b"{}"), Err(CorruptInput(0)));
        assert_eq!(std_decode(b""), Ok(Vec::new()));
        assert_eq!(std_decode(b"=AAA"), Err(CorruptInput(0)));
        assert_eq!(std_decode(b"AA=A"), Err(CorruptInput(2)));
        assert_eq!(std_decode(b"AA=\n="), Ok(vec![0]));
        assert_eq!(std_decode(b"AA==\n\n"), Ok(vec![0]));
        assert_eq!(std_decode(b"AAA=\nA"), Err(CorruptInput(5)));
    }

    #[test]
    fn encodings() {
        assert_eq!(std_encode(b"a:b?"), "YTpiPw==");
        assert_eq!(url_encode(&[0xfb, 0xff]), "-_8=");
        assert_eq!(raw_url_encode(&[0xfb, 0xff]), "-_8");
    }
}
