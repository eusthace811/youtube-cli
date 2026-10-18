//! Port of pkg/liveChatModerator and cmd/liveChatModerator.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get live chat moderator";

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
    }
    Box::new(Bound(verb, m))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let m: LiveChatModerator = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, m)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "liveChatModerator list",
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
}
