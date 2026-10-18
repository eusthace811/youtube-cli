//! Port of pkg/liveStream and cmd/liveStream.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::live_broadcast::shared_parts;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get live stream";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveStream {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub mine: Option<bool>,
    pub frame_rate: String,
    pub ingestion_type: String,
    pub resolution: String,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, LiveStream);

const LAST_PARTS: &[&str] = &["id", "snippet", "cdn", "status"];

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut s = LiveStream::default();
    s.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
    s.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
    s.fields.output = f.string("output");
    match verb {
        Verb::List => {
            s.fields.ids = f.strings("ids");
            s.mine = f.changed("mine").then(|| f.bool("mine"));
            s.fields.max_results = cli_max_results(f.i64("maxResults"));
            s.fields.parts = shared_parts(f, LAST_PARTS);
        }
    }
    Box::new(Bound(verb, s))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let s: LiveStream = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, s)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "liveStream list",
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
        let s = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => s.list(ctx).await,
            }
        })
    }
}

impl LiveStream {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.liveStreams.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            if self.mine == Some(true) {
                c = c.param("mine", true);
            }
            self.owner_params(c)
        };
        Ok(paginate(self.fields.max_results, call, ERR_GET).await)
    }

    fn owner_params<'s>(&self, mut c: yutu_api::Call<'s>) -> yutu_api::Call<'s> {
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            c = c.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        if !self.on_behalf_of_content_owner_channel.is_empty() {
            c = c.param(
                "onBehalfOfContentOwnerChannel",
                &self.on_behalf_of_content_owner_channel,
            );
        }
        c
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let fetched = self.get(ctx).await?;
        finish_list(fetched, |items| {
            print_list(
                &mut ctx.out,
                &self.fields.output,
                items,
                &["ID", "Title", "Status"],
                |s| {
                    vec![
                        Cell::from(s.str("id")),
                        Cell::from(s.str("snippet.title")),
                        Cell::from(s.str("status.streamStatus")),
                    ]
                },
            )
        })
    }
}
