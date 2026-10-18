//! Port of pkg/member and cmd/member.

use super::common::{Fields, cli_max_results};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::Cell;
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get member";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Member {
    #[serde(flatten)]
    pub fields: Fields,
    pub member_channel_id: String,
    pub has_access_to_level: String,
    pub mode: String,
}

impl Member {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.members.list")
                .params("part", &self.fields.parts);
            for (name, value) in [
                ("filterByMemberChannelId", &self.member_channel_id),
                ("hasAccessToLevel", &self.has_access_to_level),
                ("mode", &self.mode),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            c
        };
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output,
            &["Channel ID", "Display Name"],
            |m| {
                vec![
                    Cell::from(m.str("snippet.memberDetails.channelId")),
                    Cell::from(m.str("snippet.memberDetails.displayName")),
                ]
            },
        );
        let err = super::common::paginate_to(self.fields.max_results, call, ERR_GET, &mut sink)
            .await
            .1;
        sink.finish(err)
    }
}

impl Invoke for Member {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(Member {
        fields: Fields {
            max_results: cli_max_results(f.i64("maxResults")),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
        member_channel_id: f.string("memberChannelId"),
        has_access_to_level: f.string("hasAccessToLevel"),
        mode: f.string("mode"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<Member>(args)
        .map(|m| Box::new(m) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "member list",
    cli,
    mcp,
}];
