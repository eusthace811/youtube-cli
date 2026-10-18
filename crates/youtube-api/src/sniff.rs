//! Go's `http.DetectContentType` (the WHATWG MIME sniffing algorithm), which
//! the Go client uses to label uploaded media.

enum Sig {
    Html(&'static [u8]),
    Masked {
        mask: &'static [u8],
        pat: &'static [u8],
        skip_ws: bool,
        ct: &'static str,
    },
    Exact(&'static [u8], &'static str),
    Mp4,
    Text,
}

const SIGNATURES: &[Sig] = &[
    Sig::Html(b"<!DOCTYPE HTML"),
    Sig::Html(b"<HTML"),
    Sig::Html(b"<HEAD"),
    Sig::Html(b"<SCRIPT"),
    Sig::Html(b"<IFRAME"),
    Sig::Html(b"<H1"),
    Sig::Html(b"<DIV"),
    Sig::Html(b"<FONT"),
    Sig::Html(b"<TABLE"),
    Sig::Html(b"<A"),
    Sig::Html(b"<STYLE"),
    Sig::Html(b"<TITLE"),
    Sig::Html(b"<B"),
    Sig::Html(b"<BODY"),
    Sig::Html(b"<BR"),
    Sig::Html(b"<P"),
    Sig::Html(b"<!--"),
    Sig::Masked { mask: b"\xFF\xFF\xFF\xFF\xFF", pat: b"<?xml", skip_ws: true, ct: "text/xml; charset=utf-8" },
    Sig::Exact(b"%PDF-", "application/pdf"),
    Sig::Exact(b"%!PS-Adobe-", "application/postscript"),
    Sig::Masked { mask: b"\xFF\xFF\x00\x00", pat: b"\xFE\xFF\x00\x00", skip_ws: false, ct: "text/plain; charset=utf-16be" },
    Sig::Masked { mask: b"\xFF\xFF\x00\x00", pat: b"\xFF\xFE\x00\x00", skip_ws: false, ct: "text/plain; charset=utf-16le" },
    Sig::Masked { mask: b"\xFF\xFF\xFF\x00", pat: b"\xEF\xBB\xBF\x00", skip_ws: false, ct: "text/plain; charset=utf-8" },
    Sig::Exact(b"\x00\x00\x01\x00", "image/x-icon"),
    Sig::Exact(b"\x00\x00\x02\x00", "image/x-icon"),
    Sig::Exact(b"BM", "image/bmp"),
    Sig::Exact(b"GIF87a", "image/gif"),
    Sig::Exact(b"GIF89a", "image/gif"),
    Sig::Masked {
        mask: b"\xFF\xFF\xFF\xFF\x00\x00\x00\x00\xFF\xFF\xFF\xFF\xFF\xFF",
        pat: b"RIFF\x00\x00\x00\x00WEBPVP",
        skip_ws: false,
        ct: "image/webp",
    },
    Sig::Exact(b"\x89PNG\x0D\x0A\x1A\x0A", "image/png"),
    Sig::Exact(b"\xFF\xD8\xFF", "image/jpeg"),
    Sig::Masked { mask: b"\xFF\xFF\xFF\xFF\x00\x00\x00\x00\xFF\xFF\xFF\xFF", pat: b"FORM\x00\x00\x00\x00AIFF", skip_ws: false, ct: "audio/aiff" },
    Sig::Masked { mask: b"\xFF\xFF\xFF", pat: b"ID3", skip_ws: false, ct: "audio/mpeg" },
    Sig::Masked { mask: b"\xFF\xFF\xFF\xFF\xFF", pat: b"OggS\x00", skip_ws: false, ct: "application/ogg" },
    Sig::Masked { mask: b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF", pat: b"MThd\x00\x00\x00\x06", skip_ws: false, ct: "audio/midi" },
    Sig::Masked { mask: b"\xFF\xFF\xFF\xFF\x00\x00\x00\x00\xFF\xFF\xFF\xFF", pat: b"RIFF\x00\x00\x00\x00AVI ", skip_ws: false, ct: "video/avi" },
    Sig::Masked { mask: b"\xFF\xFF\xFF\xFF\x00\x00\x00\x00\xFF\xFF\xFF\xFF", pat: b"RIFF\x00\x00\x00\x00WAVE", skip_ws: false, ct: "audio/wave" },
    Sig::Mp4,
    Sig::Exact(b"\x1A\x45\xDF\xA3", "video/webm"),
    Sig::Masked {
        mask: b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xFF\xFF",
        pat: b"\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00LP",
        skip_ws: false,
        ct: "application/vnd.ms-fontobject",
    },
    Sig::Exact(b"\x00\x01\x00\x00", "font/ttf"),
    Sig::Exact(b"OTTO", "font/otf"),
    Sig::Exact(b"ttcf", "font/collection"),
    Sig::Exact(b"wOFF", "font/woff"),
    Sig::Exact(b"wOF2", "font/woff2"),
    Sig::Exact(b"\x1F\x8B\x08", "application/x-gzip"),
    Sig::Exact(b"PK\x03\x04", "application/zip"),
    Sig::Exact(b"Rar!\x1A\x07\x00", "application/x-rar-compressed"),
    Sig::Exact(b"Rar!\x1A\x07\x01\x00", "application/x-rar-compressed"),
    Sig::Exact(b"\x00\x61\x73\x6D", "application/wasm"),
    Sig::Text,
];

fn is_ws(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

pub fn detect_content_type(data: &[u8]) -> &'static str {
    let data = &data[..data.len().min(512)];
    let first = data.iter().position(|&b| !is_ws(b)).unwrap_or(data.len());
    SIGNATURES
        .iter()
        .find_map(|s| matches(s, data, first))
        .unwrap_or("application/octet-stream")
}

fn matches(sig: &Sig, data: &[u8], first: usize) -> Option<&'static str> {
    match sig {
        Sig::Exact(p, ct) => data.starts_with(p).then_some(*ct),
        Sig::Masked {
            mask,
            pat,
            skip_ws,
            ct,
        } => {
            let d = if *skip_ws { &data[first..] } else { data };
            (d.len() >= pat.len()
                && pat
                    .iter()
                    .zip(*mask)
                    .enumerate()
                    .all(|(i, (p, m))| d[i] & m == *p))
            .then_some(*ct)
        }
        Sig::Html(h) => {
            let d = &data[first..];
            if d.len() < h.len() + 1 {
                return None;
            }
            let ok = h.iter().enumerate().all(|(i, &b)| {
                let db = if b.is_ascii_uppercase() {
                    d[i] & 0xDF
                } else {
                    d[i]
                };
                b == db
            });
            (ok && matches!(d[h.len()], b' ' | b'>')).then_some("text/html; charset=utf-8")
        }
        Sig::Mp4 => {
            if data.len() < 12 {
                return None;
            }
            let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
            if data.len() < size || !size.is_multiple_of(4) || &data[4..8] != b"ftyp" {
                return None;
            }
            (8..size)
                .step_by(4)
                .filter(|&st| st != 12)
                .any(|st| &data[st..st + 3] == b"mp4")
                .then_some("video/mp4")
        }
        Sig::Text => data[first..]
            .iter()
            .all(|&b| {
                !(b <= 0x08
                    || b == 0x0B
                    || (0x0E..=0x1A).contains(&b)
                    || (0x1C..=0x1F).contains(&b))
            })
            .then_some("text/plain; charset=utf-8"),
    }
}

#[cfg(test)]
mod tests {
    use super::detect_content_type;

    #[test]
    fn sniffs_like_go() {
        assert_eq!(
            detect_content_type(b"\x89PNG\x0D\x0A\x1A\x0A rest"),
            "image/png"
        );
        assert_eq!(
            detect_content_type(b"1\n00:00:00,000 --> 00:00:01,000\nHello\n"),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            detect_content_type(b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isomiso2"),
            "application/octet-stream"
        );
        assert_eq!(
            detect_content_type(b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom"),
            "video/mp4"
        );
        assert_eq!(
            detect_content_type(b"  <html><body>"),
            "text/html; charset=utf-8"
        );
        assert_eq!(detect_content_type(b""), "text/plain; charset=utf-8");
    }
}
