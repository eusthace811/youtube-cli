//! Port of pkg/playlistItem and cmd/playlistItem.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::PLAYLIST_ITEM;

const ERR_GET: &str = "failed to get playlist item";
const ERR_INSERT: &str = "failed to insert playlist item";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlaylistItem {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub kind: String,
    pub k_video_id: String,
    pub k_channel_id: String,
    pub k_playlist_id: String,
    pub video_id: String,
    pub playlist_id: String,
    pub privacy: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, PlaylistItem);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut pi = PlaylistItem::default();
    match verb {
        Verb::List => {
            pi.fields.ids = f.strings("ids");
            pi.playlist_id = f.string("playlistId");
            pi.fields.max_results = cli_max_results(f.i64("maxResults"));
            pi.video_id = f.string("videoId");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            pi.fields.parts = f.strings("parts");
            pi.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, pi))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let pi: PlaylistItem = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, pi)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "playlistItem list",
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

impl PlaylistItem {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.playlistItems.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            for (name, value) in [
                ("playlistId", &self.playlist_id),
                (
                    "onBehalfOfContentOwner",
                    &self.fields.on_behalf_of_content_owner,
                ),
                ("videoId", &self.video_id),
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
                &["ID", "Title", "Kind", "Resource ID"],
                |item| {
                    vec![
                        Cell::from(item.str("id")),
                        Cell::from(item.str("snippet.title")),
                        Cell::from(item.str("snippet.resourceId.kind")),
                        Cell::from(resource_id(item, "snippet.resourceId")),
                    ]
                },
            )
        })
    }

    pub async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut item = Value::new_struct(PLAYLIST_ITEM);
        {
            let s = item.ensure("snippet");
            s.set_str("title", &self.title)
                .set_str("playlistId", &self.playlist_id)
                .set_str("channelId", &self.fields.channel_id);
            if let Some(d) = &self.description {
                s.set_str("description", d);
            }
        }
        let resource = match self.kind.as_str() {
            "video" => Some(("youtube#video", "videoId", &self.k_video_id)),
            "channel" => Some(("youtube#channel", "channelId", &self.k_channel_id)),
            "playlist" => Some(("youtube#playlist", "playlistId", &self.k_playlist_id)),
            _ => None,
        };
        if let Some((kind, key, id)) = resource {
            item.ensure("snippet.resourceId")
                .set_str("kind", kind)
                .set_str(key, id);
        }
        item.ensure("status")
            .set_str("privacyStatus", &self.privacy);
        let mut call = svc
            .call("youtube.playlistItems.insert")
            .params("part", &["snippet".into(), "status".into()])
            .body(&item);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Playlist Item inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }
}

/// The id a `ResourceId` holds for its kind, "" for any other kind.
pub(crate) fn resource_id<'v>(item: &'v Value, at: &str) -> &'v str {
    let field = match item.str(&format!("{at}.kind")) {
        "youtube#video" => "videoId",
        "youtube#channel" => "channelId",
        "youtube#playlist" => "playlistId",
        _ => return "",
    };
    item.str(&format!("{at}.{field}"))
}
