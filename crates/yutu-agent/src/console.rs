//! ADK's console launcher: a `User -> ` / `Agent -> ` REPL over stdin and
//! stdout, plus the approval prompt divergence E9 adds for writes.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::time::Instant;
use tokio::sync::mpsc;

use crate::agent::{Agent, BoxFuture, Frontend, Verdict};
use crate::launcher::Streaming;
use crate::llm::Meta;
use crate::model::{Content, Part, Role};
use crate::tools::Approval;

/// A line read from stdin, with its newline, and when the read that
/// completed it returned.
pub struct Line {
    pub text: String,
    pub at: Instant,
}

impl Line {
    pub fn now(text: impl Into<String>) -> Line {
        Line {
            text: text.into(),
            at: Instant::now(),
        }
    }
}

/// Lines read from stdin; a closed channel is EOF.
pub type Lines = mpsc::UnboundedReceiver<Line>;

/// Reads stdin on its own thread, as ADK's console does on a goroutine. A
/// last line without a newline is dropped, like Go's `ReadString('\n')`
/// returning it alongside EOF. Reads are unbuffered and each line carries
/// the time the kernel handed it over, so an approval can tell a line typed
/// after its question from one that was already waiting.
pub fn stdin_lines() -> Lines {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let Ok(fd) = std::io::stdin().as_fd().try_clone_to_owned() else {
            return;
        };
        let mut stdin = std::fs::File::from(fd);
        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = match stdin.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            let at = Instant::now();
            pending.extend_from_slice(&buf[..n]);
            while let Some(end) = pending.iter().position(|&b| b == b'\n') {
                let bytes: Vec<u8> = pending.drain(..=end).collect();
                let Ok(text) = String::from_utf8(bytes) else {
                    return;
                };
                if tx.send(Line { text, at }).is_err() {
                    return;
                }
            }
        }
    });
    rx
}

struct Console<'a> {
    out: &'a mut (dyn Write + Send),
    lines: &'a mut Lines,
    /// Lines queued before an approval prompt, kept for the REPL.
    ahead: VecDeque<String>,
    stream: bool,
    interactive: bool,
}

impl Console<'_> {
    fn print(&mut self, s: &str) {
        let _ = self.out.write_all(s.as_bytes());
        let _ = self.out.flush();
    }

    /// Discards terminal input until none has arrived for 150 ms, so the
    /// tail of a paste cannot answer the question about to be asked; lines
    /// already read stay queued for the REPL. False when input never stops
    /// within 2 s.
    async fn settle(&mut self) -> bool {
        use std::io::IsTerminal;
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return true;
        }
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        let mut quiet = 0;
        while Instant::now() < deadline {
            let _ = rustix::termios::tcflush(&stdin, rustix::termios::QueueSelector::IFlush);
            let mut arrived = false;
            while let Ok(line) = self.lines.try_recv() {
                self.ahead.push_back(line.text);
                arrived = true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let pending = rustix::io::ioctl_fionread(&stdin).unwrap_or(0) > 0;
            quiet = if arrived || pending { 0 } else { quiet + 1 };
            if quiet >= 3 {
                return true;
            }
        }
        false
    }

    async fn next_line(&mut self) -> Option<String> {
        match self.ahead.pop_front() {
            Some(line) => Some(line),
            None => self.lines.recv().await.map(|l| l.text),
        }
    }
}

impl Frontend for Console<'_> {
    fn chunk(&mut self, chunk: &Content, _: &Meta) {
        for part in &chunk.parts {
            if let Part::Text { text, .. } = part
                && !text.is_empty()
            {
                self.print(text);
            }
        }
    }

    fn content(&mut self, content: &Content, _: Option<&Meta>) {
        if !self.stream && content.role == Role::Model {
            self.print(&content.text());
        }
    }

    fn approve<'a>(&'a mut self, a: &'a Approval, _: usize) -> BoxFuture<'a, Verdict> {
        Box::pin(async move {
            let question = if a.message.is_empty() {
                format!("Run {}?", a.tool)
            } else {
                a.message.clone()
            };
            // Like the CLI gate, only a person at a terminal can approve, and
            // only with a line that arrived after the question was shown.
            // Input the terminal still holds is discarded, as sudo does;
            // lines that arrived earlier are kept for the REPL.
            if self.interactive && !self.settle().await {
                self.print("\nTool call needs approval, but input is still arriving: declined\n");
                return Verdict::Decline;
            }
            let asked = Instant::now();
            self.print(&format!(
                "\nTool call needs approval: {} {}\n{question} [y/N] ",
                a.tool, a.args
            ));
            if !self.interactive {
                self.print("\nnot a terminal, declined\n");
                return Verdict::Decline;
            }
            loop {
                match self.lines.recv().await {
                    Some(line) if line.at < asked => self.ahead.push_back(line.text),
                    Some(line) if matches!(line.text.trim(), "y" | "Y") => {
                        return Verdict::Approve;
                    }
                    _ => return Verdict::Decline,
                }
            }
        })
    }
}

/// Runs the REPL until EOF. `stream` is the resolved streaming mode;
/// `interactive` says stdin is a terminal a person can answer approvals on.
pub async fn run(
    agent: &Agent,
    mut lines: Lines,
    out: &mut (dyn Write + Send),
    stream: Streaming,
    interactive: bool,
) {
    let mut history: Vec<Content> = Vec::new();
    let mut console = Console {
        out,
        lines: &mut lines,
        ahead: VecDeque::new(),
        stream: stream == Streaming::Sse,
        interactive,
    };
    console.print("\n\nUser -> ");
    loop {
        let Some(line) = console.next_line().await else {
            console.print("\nEOF detected, exiting...\n");
            return;
        };
        let input = line.trim_end_matches(['\r', '\n']);
        console.print("\nAgent -> ");
        let message = Content::user_text(input);
        if let Err(e) = agent
            .turn(&mut history, message, console.stream, &mut console)
            .await
        {
            console.print(&format!("\nAGENT_ERROR: {e}\n"));
        }
        console.print("\nUser -> ");
    }
}
