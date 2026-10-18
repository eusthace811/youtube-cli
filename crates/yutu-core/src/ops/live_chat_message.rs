//! Port of pkg/liveChatMessage and cmd/liveChatMessage.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::live_broadcast::shared_parts;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get live chat message";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveChatMessage {
    #[serde(flatten)]
    pub fields: Fields,
    pub live_chat_id: String,
    pub message_text: String,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, LiveChatMessage);

const LAST_PARTS: &[&str] = &["snippet", "authorDetails"];

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut m = LiveChatMessage::default();
    m.fields.output = f.string("output");
    match verb {
        Verb::List => {
            m.live_chat_id = f.string("liveChatId");
            m.fields.hl = f.string("hl");
            m.fields.max_results = cli_max_results(f.i64("maxResults"));
            m.fields.parts = shared_parts(f, LAST_PARTS);
        }
    }
    Box::new(Bound(verb, m))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let m: LiveChatMessage = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, m)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "liveChatMessage list",
    cli: |f| cli(Verb::List, f),
    mcp: |a| mcp(Verb::List, a),
}];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        match self.0 {
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let m = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => m.list(ctx).await,
            }
        })
    }
}

impl LiveChatMessage {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.liveChatMessages.list")
                .param("liveChatId", &self.live_chat_id)
                .params("part", &self.fields.parts);
            if !self.fields.hl.is_empty() {
                c = c.param("hl", &self.fields.hl);
            }
            c
        };
        let fetched = paginate(self.fields.max_results, call, ERR_GET).await;
        finish_list(fetched, |items| {
            print_list(
                &mut ctx.out,
                &self.fields.output,
                items,
                &["ID", "Type", "Author", "Message"],
                |msg| {
                    vec![
                        Cell::from(msg.str("id")),
                        Cell::from(msg.str("snippet.type")),
                        Cell::from(msg.str("authorDetails.displayName")),
                        Cell::from(msg.str("snippet.displayMessage")),
                    ]
                },
            )
        })
    }
}
