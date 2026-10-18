//! ADK's console launcher: a `User -> ` / `Agent -> ` REPL over stdin and
//! stdout, plus the approval prompt divergence E9 adds for writes.

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use tokio::sync::mpsc;

use crate::agent::{Agent, BoxFuture, Frontend, Verdict};
use crate::launcher::Streaming;
use crate::llm::Meta;
use crate::model::{Content, Part, Role};
use crate::tools::Approval;

/// Lines read from stdin, each with its newline; a closed channel is EOF.
pub type Lines = mpsc::UnboundedReceiver<String>;

/// Reads stdin on its own thread, as ADK's console does on a goroutine. A
/// last line without a newline is dropped, like Go's `ReadString('\n')`
/// returning it alongside EOF. The thread reads eagerly, so whatever was
/// typed or pasted before an approval prompt is already queued when the
/// prompt appears and cannot answer it.
pub fn stdin_lines() -> Lines {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lock = stdin.lock();
        loop {
            let mut line = String::new();
            match lock.read_line(&mut line) {
                Ok(n) if n > 0 && line.ends_with('\n') => {
                    if tx.send(line).is_err() {
                        return;
                    }
                }
                _ => return,
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

    async fn next_line(&mut self) -> Option<String> {
        match self.ahead.pop_front() {
            Some(line) => Some(line),
            None => self.lines.recv().await,
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
            // only with a line entered after seeing the question: whatever
            // is queued before the question is printed is kept for the REPL.
            while let Ok(line) = self.lines.try_recv() {
                self.ahead.push_back(line);
            }
            self.print(&format!(
                "\nTool call needs approval: {} {}\n{question} [y/N] ",
                a.tool, a.args
            ));
            if !self.interactive {
                self.print("\nnot a terminal, declined\n");
                return Verdict::Decline;
            }
            match self.lines.recv().await.as_deref().map(str::trim) {
                Some("y" | "Y") => Verdict::Approve,
                _ => Verdict::Decline,
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
