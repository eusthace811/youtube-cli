//! ADK's console launcher: a `User -> ` / `Agent -> ` REPL over stdin and
//! stdout, plus the approval prompt divergence E9 adds for writes.

use std::io::{BufRead, Write};
use tokio::sync::mpsc;

use crate::agent::{Agent, BoxFuture, Frontend};
use crate::launcher::Streaming;
use crate::model::Content;
use crate::tools::Approval;

/// Lines read from stdin, each with its newline; a closed channel is EOF.
pub type Lines = mpsc::Receiver<String>;

/// Reads stdin on its own thread, as ADK's console does on a goroutine. A
/// last line without a newline is dropped, like Go's `ReadString('\n')`
/// returning it alongside EOF.
pub fn stdin_lines() -> Lines {
    let (tx, rx) = mpsc::channel(1);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lock = stdin.lock();
        loop {
            let mut line = String::new();
            match lock.read_line(&mut line) {
                Ok(n) if n > 0 && line.ends_with('\n') => {
                    if tx.blocking_send(line).is_err() {
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
}

impl Console<'_> {
    fn print(&mut self, s: &str) {
        let _ = self.out.write_all(s.as_bytes());
        let _ = self.out.flush();
    }
}

impl Frontend for Console<'_> {
    fn text(&mut self, text: &str) {
        self.print(text);
    }

    fn approve<'a>(&'a mut self, a: &'a Approval) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let question = if a.message.is_empty() {
                format!("Run {}?", a.tool)
            } else {
                a.message.clone()
            };
            self.print(&format!(
                "\nTool call needs approval: {} {}\n{question} [y/N] ",
                a.tool, a.args
            ));
            matches!(
                self.lines.recv().await.as_deref().map(str::trim),
                Some("y" | "Y")
            )
        })
    }
}

/// Runs the REPL until EOF. `stream` is the resolved streaming mode.
pub async fn run(agent: &Agent, mut lines: Lines, out: &mut (dyn Write + Send), stream: Streaming) {
    let mut history: Vec<Content> = Vec::new();
    let mut console = Console {
        out,
        lines: &mut lines,
    };
    console.print("\n\nUser -> ");
    loop {
        let Some(line) = console.lines.recv().await else {
            console.print("\nEOF detected, exiting...\n");
            return;
        };
        let input = line.trim_end_matches(['\r', '\n']);
        console.print("\nAgent -> ");
        let message = Content::user_text(input);
        if let Err(e) = agent
            .turn(
                &mut history,
                message,
                stream == Streaming::Sse,
                &mut console,
            )
            .await
        {
            console.print(&format!("\nAGENT_ERROR: {e}\n"));
        }
        console.print("\nUser -> ");
    }
}
