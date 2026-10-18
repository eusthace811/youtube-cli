//! Port of pkg/activity and cmd/activity.

use super::common::{Fields, cli_max_results};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::Cell;
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get activity";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Activity {
    #[serde(flatten)]
    pub fields: Fields,
    #[serde(rename = "for")]
    pub for_: String,
    pub published_after: String,
    pub published_before: String,
    pub region_code: String,
}

impl Activity {
    pub async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.activities.list")
                .params("part", &self.fields.parts);
            if !self.fields.channel_id.is_empty() {
                c = c.param("channelId", &self.fields.channel_id);
            }
            match self.for_.as_str() {
                "home" => c = c.param("home", true),
                "mine" => c = c.param("mine", true),
                _ => {}
            }
            for (name, value) in [
                ("publishedAfter", &self.published_after),
                ("publishedBefore", &self.published_before),
                ("regionCode", &self.region_code),
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
            &["ID", "Title", "Type", "Time"],
            |a| {
                vec![
                    Cell::from(a.str("id")),
                    Cell::from(a.str("snippet.title")),
                    Cell::from(a.str("snippet.type")),
                    Cell::from(a.str("snippet.publishedAt")),
                ]
            },
        );
        let err = super::common::paginate_to(self.fields.max_results, call, ERR_GET, &mut sink)
            .await
            .1;
        sink.finish(err)
    }
}

impl Invoke for Activity {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(Activity {
        fields: Fields {
            channel_id: f.string("channelId"),
            max_results: cli_max_results(f.i64("maxResults")),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
        for_: f.string("for"),
        published_after: f.string("publishedAfter"),
        published_before: f.string("publishedBefore"),
        region_code: f.string("regionCode"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<Activity>(args)
        .map(|a| Box::new(a) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "activity list",
    cli,
    mcp,
}];
