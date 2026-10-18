//! Resumable uploads for media above the single-request limit (divergence
//! E7): one session, then 8 MiB chunks streamed from disk.

use crate::client::{Call, Media, RESUMABLE_CHUNK, canonical_header, check, transport};
use crate::error::Error;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

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
        .header("X-Upload-Content-Length", size.to_string())
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
    let mut offset = 0u64;
    let mut file = match media {
        Media::File(p) => Some(
            tokio::fs::File::open(p)
                .await
                .map_err(|e| Error::Local(e.to_string()))?,
        ),
        Media::Bytes(_) => None,
    };
    loop {
        let len = RESUMABLE_CHUNK.min(size - offset);
        let mut chunk = vec![0u8; len as usize];
        match (&mut file, media) {
            (Some(f), _) => {
                f.seek(std::io::SeekFrom::Start(offset))
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?;
                f.read_exact(&mut chunk)
                    .await
                    .map_err(|e| Error::Local(e.to_string()))?;
            }
            (None, Media::Bytes(b)) => {
                chunk.copy_from_slice(&b[offset as usize..(offset + len) as usize])
            }
            (None, Media::File(_)) => unreachable!(),
        }
        let range = if len == 0 {
            format!("bytes */{size}")
        } else {
            format!("bytes {offset}-{}/{size}", offset + len - 1)
        };
        let resp = call
            .authorized("PUT", &session)
            .await?
            .header("Content-Range", range)
            .header("Content-Type", ctype)
            .body(chunk)
            .send()
            .await
            .map_err(|e| transport("PUT", &session, e))?;
        if resp.status().as_u16() != 308 {
            return check(resp).await;
        }
        offset = match resp
            .headers()
            .get(canonical_header("range").as_str())
            .and_then(|v| v.to_str().ok())
        {
            Some(r) => r
                .rsplit('-')
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .map(|n| n + 1)
                .unwrap_or(offset + len),
            None => 0,
        };
        if offset >= size {
            return Err(Error::Local(
                "resumable upload: server acknowledged all bytes without completing".into(),
            ));
        }
    }
}
