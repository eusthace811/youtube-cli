//! Port of pkg/liveChatMessage and cmd/liveChatMessage.

use super::common::{Fields, cli_max_results};
use super::live_broadcast::shared_parts;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::LIVE_CHAT_MESSAGE;

const ERR_GET: &str = "failed to get live chat message";
const ERR_INSERT: &str = "failed to insert live chat message";
const ERR_DELETE: &str = "failed to delete live chat message";
const ERR_TRANSITION: &str = "failed to transition live chat message";

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
    Insert,
    Delete,
    Transition,
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
        Verb::Insert => {
            m.live_chat_id = f.string("liveChatId");
            m.message_text = f.string("messageText");
            m.fields.parts = shared_parts(f, LAST_PARTS);
        }
        Verb::Delete => m.fields.ids = f.strings("ids"),
        Verb::Transition => {
            m.fields.ids = f.strings("ids");
            m.status = f.string("status");
        }
    }
    Box::new(Bound(verb, m))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let m: LiveChatMessage = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, m)))
}

pub static ENTRIES: [Entry; 4] = [
    Entry {
        path: "liveChatMessage delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "liveChatMessage insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "liveChatMessage list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "liveChatMessage transition",
        cli: |f| cli(Verb::Transition, f),
        mcp: |a| mcp(Verb::Transition, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let m = &self.1;
        let ids = m.fields.ids.join(", ");
        match self.0 {
            Verb::List => String::new(),
            Verb::Insert => format!("Send message to live chat {}", m.live_chat_id),
            Verb::Delete => format!("Delete live chat message(s): {ids}"),
            Verb::Transition => {
                format!("Transition live chat message(s) {ids} to {}", m.status)
            }
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let m = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => m.list(ctx).await,
                Verb::Insert => m.insert(ctx).await,
                Verb::Delete => m.delete(ctx).await,
                Verb::Transition => m.transition(ctx).await,
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
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output,
            &["ID", "Type", "Author", "Message"],
            |msg| {
                vec![
                    Cell::from(msg.str("id")),
                    Cell::from(msg.str("snippet.type")),
                    Cell::from(msg.str("authorDetails.displayName")),
                    Cell::from(msg.str("snippet.displayMessage")),
                ]
            },
        );
        let err = super::common::paginate_to(self.fields.max_results, call, ERR_GET, &mut sink)
            .await
            .1;
        sink.finish(err)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut msg = Value::new_struct(LIVE_CHAT_MESSAGE);
        msg.ensure("snippet")
            .set_str("liveChatId", &self.live_chat_id)
            .set_str("type", "textMessageEvent");
        msg.ensure("snippet.textMessageDetails")
            .set_str("messageText", &self.message_text);
        let res = svc
            .call("youtube.liveChatMessages.insert")
            .params("part", &self.fields.parts)
            .body(&msg)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Live chat message inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.liveChatMessages.delete")
                .param("id", id)
                .send_empty()
                .await
            {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx
                        .out
                        .write_str(&format!("Live chat message {id} deleted\n"))
                    {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    async fn transition(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.liveChatMessages.transition")
                .param("id", id)
                .param("status", &self.status)
                .send()
                .await
            {
                Err(e) => {
                    errs.push(ERR_TRANSITION);
                    errs.push(e.to_string());
                }
                Ok(res) => {
                    if let Err(e) = print_result(
                        &mut ctx.out,
                        &self.fields.output,
                        &res,
                        &format!("Live chat message {id} transitioned to {}\n", self.status),
                    ) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
