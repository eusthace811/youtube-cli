//! Port of pkg/playlistImage and cmd/playlistImage.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get playlist image";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlaylistImage {
    #[serde(flatten)]
    pub fields: Fields,
    pub height: i64,
    pub playlist_id: String,
    pub r#type: String,
    pub width: i64,
    pub file: String,
    pub parent: String,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, PlaylistImage);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut pi = PlaylistImage::default();
    match verb {
        Verb::List => {
            pi.parent = f.string("parent");
            pi.fields.max_results = cli_max_results(f.i64("maxResults"));
            pi.fields.parts = f.strings("parts");
            pi.fields.output = f.string("output");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            pi.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
        }
    }
    Box::new(Bound(verb, pi))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let pi: PlaylistImage = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, pi)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "playlistImage list",
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
        let pi = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => pi.list(ctx).await,
            }
        })
    }
}

impl PlaylistImage {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.playlistImages.list")
                .params("part", &self.fields.parts);
            for (name, value) in [
                ("parent", &self.parent),
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
                &["ID", "Kind", "Playlist ID", "Type"],
                |img| {
                    vec![
                        Cell::from(img.str("id")),
                        Cell::from(img.str("kind")),
                        Cell::from(img.str("snippet.playlistId")),
                        Cell::from(img.str("snippet.type")),
                    ]
                },
            )
        })
    }
}
