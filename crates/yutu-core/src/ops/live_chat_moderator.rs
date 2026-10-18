//! Port of pkg/liveChatModerator and cmd/liveChatModerator.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::LIVE_CHAT_MODERATOR;

const ERR_GET: &str = "failed to get live chat moderator";
const ERR_INSERT: &str = "failed to insert live chat moderator";
const ERR_DELETE: &str = "failed to delete live chat moderator";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveChatModerator {
    #[serde(flatten)]
    pub fields: Fields,
    pub live_chat_id: String,
    pub moderator_channel_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Insert,
    Delete,
}

struct Bound(Verb, LiveChatModerator);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut m = LiveChatModerator::default();
    match verb {
        Verb::List => {
            m.live_chat_id = f.string("liveChatId");
            m.fields.max_results = cli_max_results(f.i64("maxResults"));
            m.fields.parts = f.strings("parts");
            m.fields.output = f.string("output");
        }
        Verb::Insert => {
            m.live_chat_id = f.string("liveChatId");
            m.moderator_channel_id = f.string("moderatorChannelId");
            m.fields.parts = f.strings("parts");
            m.fields.output = f.string("output");
        }
        Verb::Delete => m.fields.ids = f.strings("ids"),
    }
    Box::new(Bound(verb, m))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let m: LiveChatModerator = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, m)))
}

pub static ENTRIES: [Entry; 3] = [
    Entry {
        path: "liveChatModerator delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "liveChatModerator insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "liveChatModerator list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let m = &self.1;
        match self.0 {
            Verb::List => String::new(),
            Verb::Insert => format!(
                "Add moderator {} to live chat {}",
                m.moderator_channel_id, m.live_chat_id
            ),
            Verb::Delete => format!("Delete live chat moderator(s): {}", m.fields.ids.join(", ")),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let m = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => m.list(ctx).await,
                Verb::Insert => m.insert(ctx).await,
                Verb::Delete => m.delete(ctx).await,
            }
        })
    }
}

impl LiveChatModerator {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            svc.call("youtube.liveChatModerators.list")
                .param("liveChatId", &self.live_chat_id)
                .params("part", &self.fields.parts)
        };
        let fetched = paginate(self.fields.max_results, call, ERR_GET).await;
        finish_list(fetched, |items| {
            print_list(
                &mut ctx.out,
                &self.fields.output,
                items,
                &["ID", "Channel ID", "Display Name"],
                |m| {
                    vec![
                        Cell::from(m.str("id")),
                        Cell::from(m.str("snippet.moderatorDetails.channelId")),
                        Cell::from(m.str("snippet.moderatorDetails.displayName")),
                    ]
                },
            )
        })
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut moderator = Value::new_struct(LIVE_CHAT_MODERATOR);
        moderator
            .ensure("snippet")
            .set_str("liveChatId", &self.live_chat_id);
        moderator
            .ensure("snippet.moderatorDetails")
            .set_str("channelId", &self.moderator_channel_id);
        let res = svc
            .call("youtube.liveChatModerators.insert")
            .params("part", &self.fields.parts)
            .body(&moderator)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Live chat moderator inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.liveChatModerators.delete")
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
                        .write_str(&format!("Live chat moderator {id} deleted\n"))
                    {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
