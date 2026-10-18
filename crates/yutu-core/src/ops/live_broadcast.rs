//! Port of pkg/liveBroadcast and cmd/liveBroadcast.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get live broadcast";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveBroadcast {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub mine: Option<bool>,
    pub broadcast_status: String,
    pub broadcast_type: String,
    pub privacy_status: String,
    pub scheduled_start_time: String,
    pub scheduled_end_time: String,
    pub stream_id: String,
    pub cue_type: String,
    pub cue_duration_secs: i64,
    pub cue_insertion_offset_ms: i64,
    pub cue_walltime_ms: u64,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, LiveBroadcast);

/// The `parts` slice is shared by every subcommand of a Go package, so an
/// unset `--parts` holds the default of the last command registering it in
/// init order, not the invoked command's own default.
pub(super) fn shared_parts(f: &dyn FlagValues, last_default: &[&str]) -> Vec<String> {
    if f.changed("parts") {
        f.strings("parts")
    } else {
        last_default.iter().map(|p| p.to_string()).collect()
    }
}

const LAST_PARTS: &[&str] = &["id", "snippet", "status"];

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut b = LiveBroadcast::default();
    b.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
    b.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
    b.fields.output = f.string("output");
    match verb {
        Verb::List => {
            b.fields.ids = f.strings("ids");
            b.mine = f.changed("mine").then(|| f.bool("mine"));
            b.broadcast_status = f.string("broadcastStatus");
            b.broadcast_type = f.string("broadcastType");
            b.fields.max_results = cli_max_results(f.i64("maxResults"));
            b.fields.parts = shared_parts(f, LAST_PARTS);
        }
    }
    Box::new(Bound(verb, b))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let b: LiveBroadcast = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, b)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "liveBroadcast list",
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
        let b = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => b.list(ctx).await,
            }
        })
    }
}

impl LiveBroadcast {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.liveBroadcasts.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            if self.mine == Some(true) {
                c = c.param("mine", true);
            }
            for (name, value) in [
                ("broadcastStatus", &self.broadcast_status),
                ("broadcastType", &self.broadcast_type),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
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
                &["ID", "Title", "Status", "Privacy"],
                |bc| {
                    vec![
                        Cell::from(bc.str("id")),
                        Cell::from(bc.str("snippet.title")),
                        Cell::from(bc.str("status.lifeCycleStatus")),
                        Cell::from(bc.str("status.privacyStatus")),
                    ]
                },
            )
        })
    }
}
