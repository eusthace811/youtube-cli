//! A server-sent events reader over a streamed response body.

use futures_util::StreamExt;

pub struct Event {
    pub name: String,
    pub data: String,
}

pub struct Reader {
    body: futures_util::stream::BoxStream<'static, reqwest::Result<bytes::Bytes>>,
    buf: Vec<u8>,
    done: bool,
}

impl Reader {
    pub fn new(resp: reqwest::Response) -> Reader {
        Reader {
            body: resp.bytes_stream().boxed(),
            buf: Vec::new(),
            done: false,
        }
    }

    /// The next event, or `None` at the end of the stream.
    pub async fn next(&mut self) -> Result<Option<Event>, String> {
        let mut name = String::new();
        let mut data: Vec<String> = Vec::new();
        loop {
            let line = match self.line().await? {
                Some(l) => l,
                None if data.is_empty() => return Ok(None),
                None => break,
            };
            if line.is_empty() {
                if data.is_empty() {
                    continue;
                }
                break;
            }
            if let Some(v) = line.strip_prefix("data:") {
                data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
            } else if let Some(v) = line.strip_prefix("event:") {
                name = v.trim().to_string();
            }
        }
        Ok(Some(Event {
            name,
            data: data.join("\n"),
        }))
    }

    async fn line(&mut self) -> Result<Option<String>, String> {
        loop {
            if let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=i).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            if self.done {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let rest = std::mem::take(&mut self.buf);
                return Ok(Some(String::from_utf8_lossy(&rest).into_owned()));
            }
            match self.body.next().await {
                Some(Ok(chunk)) => self.buf.extend_from_slice(&chunk),
                Some(Err(e)) => return Err(e.to_string()),
                None => self.done = true,
            }
        }
    }
}
