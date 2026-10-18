//! Port of pkg/playlist and cmd/playlist.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get playlist";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Playlist {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub mine: Option<bool>,
    pub tags: Option<Vec<String>>,
    pub language: Option<String>,
    pub privacy: String,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, Playlist);

/// A `*bool` that `resetFlags` nils out when the flag was not set.
fn reset_bool(f: &dyn FlagValues, name: &str) -> Option<bool> {
    f.changed(name).then(|| f.bool(name))
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut p = Playlist::default();
    match verb {
        Verb::List => {
            p.fields.ids = f.strings("ids");
            p.fields.channel_id = f.string("channelId");
            p.fields.hl = f.string("hl");
            p.fields.max_results = cli_max_results(f.i64("maxResults"));
            p.mine = reset_bool(f, "mine");
            p.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            p.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
            p.fields.parts = f.strings("parts");
            p.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, p))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let p: Playlist = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, p)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "playlist list",
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
        let p = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => p.list(ctx).await,
            }
        })
    }
}

impl Playlist {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.playlists.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            for (name, value) in [
                ("channelId", &self.fields.channel_id),
                ("hl", &self.fields.hl),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            if let Some(m) = self.mine {
                c = c.param("mine", m);
            }
            for (name, value) in [
                (
                    "onBehalfOfContentOwner",
                    &self.fields.on_behalf_of_content_owner,
                ),
                (
                    "onBehalfOfContentOwnerChannel",
                    &self.on_behalf_of_content_owner_channel,
                ),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            c
        };
        Ok(paginate(self.fields.max_results, call, ERR_GET).await)
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let fetched = self.get(ctx).await?;
        let output = self.fields.output.clone();
        finish_list(fetched, |items| {
            print_list(
                &mut ctx.out,
                &output,
                items,
                &["ID", "Channel ID", "Title"],
                |p| {
                    vec![
                        Cell::from(p.str("id")),
                        Cell::from(p.str("snippet.channelId")),
                        Cell::from(p.str("snippet.title")),
                    ]
                },
            )
        })
    }
}
