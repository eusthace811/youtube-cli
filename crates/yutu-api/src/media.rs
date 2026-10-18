//! Resumable uploads for media above the single-request limit (divergence
//! E7): one session, then 8 MiB chunks streamed from disk. Chunks go out the
//! way the Go client sends them: POST with `X-GUploader-No-308: yes`, so the
//! server answers "resume incomplete" as 200 plus an override header instead
//! of a bare 308. A failed or interrupted chunk is retried with backoff after
//! asking the server how many bytes it already holds.

use crate::client::{Call, Media, RESUMABLE_CHUNK, check, transport};
use crate::error::Error;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

/// Attempts allowed without the server acknowledging new bytes.
const MAX_STALLED_ATTEMPTS: u32 = 6;

pub(crate) async fn resumable(
    call: &Call<'_>,
    upload_path: &str,
    ctype: &str,
    size: u64,
) -> Result<reqwest::Response, Error> {
    let (url, http_method, body, media) = call.resumable_parts(upload_path);
    let start = call
        .authorized(http_method, &url)
        .await?
        .header("Content-Type", "application/json")
        .header("X-Upload-Content-Type", ctype)
        .body(body.unwrap_or_default())
        .send()
        .await
        .map_err(|e| transport(http_method, &url, e))?;
    let start = check(start).await?;
    let session = start
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::Local("resumable upload: no session location".into()))?
        .to_string();
    if !session_host_allowed(&session, &url) {
        return Err(Error::Local(format!(
            "resumable upload: refusing session on another host: {session}"
        )));
    }
    let file = media.open().await?;
    let mut offset = 0u64;
    let mut stalled = 0u32;
    loop {
        let len = RESUMABLE_CHUNK.min(size - offset);
        let body = match (&file, media) {
            (Some(f), _) => {
                let mut f = f
                    .try_clone()
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?;
                f.seek(std::io::SeekFrom::Start(offset))
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?;
                reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::with_capacity(
                    f.take(len),
                    1 << 20,
                ))
            }
            (None, Media::Bytes(b)) => {
                reqwest::Body::from(b[offset as usize..(offset + len) as usize].to_vec())
            }
            (None, _) => unreachable!(),
        };
        let range = if len == 0 {
            format!("bytes */{size}")
        } else {
            format!("bytes {offset}-{}/{size}", offset + len - 1)
        };
        let sent = call
            .authorized("POST", &session)
            .await?
            .header("Content-Range", range)
            .header("Content-Type", ctype)
            .header("Content-Length", len.to_string())
            .header("X-GUploader-No-308", "yes")
            .body(body)
            .send()
            .await;
        let next = match sent {
            Ok(resp) if incomplete(&resp) => acknowledged(&resp),
            Ok(resp) if retryable(resp.status().as_u16()) => None,
            Ok(resp) => return check(resp).await,
            Err(e) if stalled + 1 >= MAX_STALLED_ATTEMPTS => {
                return Err(transport("POST", &session, e));
            }
            Err(_) => None,
        };
        let next = match next {
            Some(n) => n,
            None => {
                tokio::time::sleep(backoff(stalled)).await;
                match status(call, &session, size).await? {
                    Status::Done(resp) => return check(resp).await,
                    Status::Offset(n) => n,
                }
            }
        };
        if next > offset {
            stalled = 0;
        } else {
            stalled += 1;
            if stalled >= MAX_STALLED_ATTEMPTS {
                return Err(Error::Local(format!(
                    "resumable upload: no progress after {MAX_STALLED_ATTEMPTS} attempts at byte {offset}"
                )));
            }
        }
        offset = next;
        if offset >= size {
            return Err(Error::Local(
                "resumable upload: server acknowledged all bytes without completing".into(),
            ));
        }
    }
}

enum Status {
    Offset(u64),
    Done(reqwest::Response),
}

/// Asks the session how many bytes it has (`Content-Range: bytes */size`),
/// retrying with backoff while the server is unreachable or busy.
async fn status(call: &Call<'_>, session: &str, size: u64) -> Result<Status, Error> {
    let mut attempt = 0;
    loop {
        let sent = call
            .authorized("POST", session)
            .await?
            .header("Content-Range", format!("bytes */{size}"))
            .header("X-GUploader-No-308", "yes")
            .send()
            .await;
        match sent {
            Ok(resp) if incomplete(&resp) => {
                return Ok(Status::Offset(acknowledged(&resp).unwrap_or(0)));
            }
            Ok(resp) if !retryable(resp.status().as_u16()) => return Ok(Status::Done(resp)),
            Ok(_) | Err(_) if attempt + 1 < MAX_STALLED_ATTEMPTS => {}
            Ok(resp) => return check(resp).await.map(Status::Done),
            Err(e) => return Err(transport("POST", session, e)),
        }
        tokio::time::sleep(backoff(attempt)).await;
        attempt += 1;
    }
}

/// "Resume incomplete": a 200 carrying `X-Http-Status-Code-Override: 308`,
/// or a bare 308 from a server that ignored `X-GUploader-No-308`.
fn incomplete(resp: &reqwest::Response) -> bool {
    resp.status().as_u16() == 308
        || resp
            .headers()
            .get("x-http-status-code-override")
            .and_then(|v| v.to_str().ok())
            == Some("308")
}

/// The next byte to send, from a `Range: bytes=0-N` header; no header means
/// the server holds nothing yet.
fn acknowledged(resp: &reqwest::Response) -> Option<u64> {
    match resp.headers().get("range").and_then(|v| v.to_str().ok()) {
        Some(r) => r
            .rsplit('-')
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .map(|n| n + 1),
        None => Some(0),
    }
}

fn retryable(status: u16) -> bool {
    status == 429 || (500..=599).contains(&status)
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64 << attempt.min(5))
}

/// The session URL must stay on the host the upload started on, or on a
/// Google API host over TLS, so a forged Location cannot collect the token.
fn session_host_allowed(session: &str, start: &str) -> bool {
    match (authority(session), authority(start)) {
        (Some((ss, sh)), Some((os, oh))) => {
            (sh == oh && ss == os) || (ss == "https" && sh.ends_with(".googleapis.com"))
        }
        _ => false,
    }
}

/// Scheme and host of an absolute URL, refusing userinfo, backslashes and
/// other authority forms a WHATWG parser would read differently.
pub(crate) fn authority(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let auth = &rest[..end];
    if auth.is_empty() || auth.contains(['@', '\\', '%', ' ']) {
        return None;
    }
    let host = match auth.rsplit_once(':') {
        Some((h, port)) if !h.ends_with(']') && port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => auth,
    };
    Some((scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::session_host_allowed;

    #[test]
    fn session_must_stay_on_a_trusted_host() {
        let start = "https://youtube.googleapis.com/upload/youtube/v3/videos?uploadType=resumable";
        assert!(session_host_allowed(
            "https://youtube.googleapis.com/upload/youtube/v3/videos?upload_id=x",
            start
        ));
        assert!(session_host_allowed(
            "https://www.googleapis.com/upload/x",
            start
        ));
        assert!(!session_host_allowed("https://evil.example/upload", start));
        assert!(!session_host_allowed(
            "http://www.googleapis.com/upload/x",
            start
        ));
        assert!(session_host_allowed(
            "http://127.0.0.1:5000/resumable/1",
            "http://127.0.0.1:5000/upload/youtube/v3/videos"
        ));
    }
}
