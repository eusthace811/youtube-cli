//! Port of pkg/superChatEvent and cmd/superChatEvent.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get super chat event";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SuperChatEvent {
    #[serde(flatten)]
    pub fields: Fields,
}

impl SuperChatEvent {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.superChatEvents.list")
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
                &["ID", "Amount", "Comment", "Supporter"],
                |e| {
                    vec![
                        Cell::from(e.str("id")),
                        Cell::from(e.str("snippet.displayString")),
                        Cell::from(e.str("snippet.commentText")),
                        Cell::from(e.str("snippet.supporterDetails.displayName")),
                    ]
                },
            )
        })
    }
}

impl Invoke for SuperChatEvent {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(SuperChatEvent {
        fields: Fields {
            hl: f.string("hl"),
            max_results: cli_max_results(f.i64("maxResults")),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<SuperChatEvent>(args)
        .map(|s| Box::new(s) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "superChatEvent list",
    cli,
    mcp,
}];
