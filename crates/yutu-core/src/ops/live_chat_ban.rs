//! Port of pkg/liveChatBan and cmd/liveChatBan.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::print_result;
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::LIVE_CHAT_BAN;

const ERR_INSERT: &str = "failed to insert live chat ban";
const ERR_DELETE: &str = "failed to delete live chat ban";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveChatBan {
    #[serde(flatten)]
    pub fields: Fields,
    pub live_chat_id: String,
    pub banned_user_channel_id: String,
    pub ban_duration_seconds: u64,
    pub ban_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    Insert,
    Delete,
}

struct Bound(Verb, LiveChatBan);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut b = LiveChatBan::default();
    match verb {
        Verb::Insert => {
            b.live_chat_id = f.string("liveChatId");
            b.banned_user_channel_id = f.string("bannedUserChannelId");
            b.ban_type = f.string("banType");
            b.ban_duration_seconds = f.u64("banDurationSeconds");
            b.fields.parts = f.strings("parts");
            b.fields.output = f.string("output");
        }
        Verb::Delete => b.fields.ids = f.strings("ids"),
    }
    Box::new(Bound(verb, b))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let b: LiveChatBan = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, b)))
}

pub static ENTRIES: [Entry; 2] = [
    Entry {
        path: "liveChatBan delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "liveChatBan insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let b = &self.1;
        match self.0 {
            Verb::Insert => format!(
                "Ban user {} in live chat {}",
                b.banned_user_channel_id, b.live_chat_id
            ),
            Verb::Delete => format!("Delete live chat ban(s): {}", b.fields.ids.join(", ")),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let b = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::Insert => b.insert(ctx).await,
                Verb::Delete => b.delete(ctx).await,
            }
        })
    }
}

impl LiveChatBan {
    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut ban = Value::new_struct(LIVE_CHAT_BAN);
        ban.ensure("snippet")
            .set_str("liveChatId", &self.live_chat_id)
            .set_str("type", &self.ban_type)
            .set("banDurationSeconds", Value::Uint(self.ban_duration_seconds));
        ban.ensure("snippet.bannedUserDetails")
            .set_str("channelId", &self.banned_user_channel_id);
        let res = svc
            .call("youtube.liveChatBans.insert")
            .params("part", &self.fields.parts)
            .body(&ban)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Live chat ban inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.liveChatBans.delete")
                .param("id", id)
                .send_empty()
                .await
            {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Live chat ban {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
