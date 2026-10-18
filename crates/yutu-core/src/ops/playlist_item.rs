//! Port of pkg/playlistItem and cmd/playlistItem.

use super::common::{Fields, cli_max_results};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::PLAYLIST_ITEM;

const ERR_GET: &str = "failed to get playlist item";
const ERR_INSERT: &str = "failed to insert playlist item";
const ERR_UPDATE: &str = "failed to update playlist item";
const ERR_DELETE: &str = "failed to delete playlist item";

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
    Insert,
    Update,
    Delete,
}

struct Bound(Verb, PlaylistItem);

/// The `*string` description that `resetFlags` nils out when unset.
fn reset_string(f: &dyn FlagValues, name: &str) -> Option<String> {
    f.changed(name).then(|| f.string(name))
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut pi = PlaylistItem::default();
    match verb {
        Verb::Insert => {
            pi.title = f.string("title");
            pi.description = reset_string(f, "description");
            pi.kind = f.string("kind");
            pi.k_video_id = f.string("kVideoId");
            pi.k_channel_id = f.string("kChannelId");
            pi.k_playlist_id = f.string("kPlaylistId");
            pi.playlist_id = f.string("playlistId");
            pi.privacy = f.string("privacy");
            pi.fields.channel_id = f.string("channelId");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            pi.fields.output = f.string("output");
        }
        Verb::Update => {
            pi.fields.ids = f.strings("id");
            pi.title = f.string("title");
            pi.description = reset_string(f, "description");
            pi.privacy = f.string("privacy");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            pi.fields.output = f.string("output");
        }
        Verb::Delete => {
            pi.fields.ids = f.strings("ids");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
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

pub static ENTRIES: [Entry; 4] = [
    Entry {
        path: "playlistItem delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "playlistItem insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "playlistItem list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "playlistItem update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let pi = &self.1;
        let ids = pi.fields.ids.join(", ");
        match self.0 {
            Verb::Delete => format!("Delete playlist item(s): {ids}"),
            Verb::Insert => format!("Add playlist item to playlist {}", pi.playlist_id),
            Verb::Update => format!("Update playlist item: {ids}"),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let pi = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => pi.list(ctx).await,
                Verb::Insert => pi.insert(ctx).await,
                Verb::Update => pi.update(ctx).await,
                Verb::Delete => pi.delete(ctx).await,
            }
        })
    }
}

impl PlaylistItem {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let mut all = super::common::Collect::default();
        let err = self.pages(&svc, &mut all).await;
        Ok((all.items, err))
    }

    async fn pages(
        &self,
        svc: &yutu_api::Service,
        sink: &mut dyn super::common::PageSink,
    ) -> Option<OpError> {
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
        super::common::paginate_to(self.fields.max_results, call, ERR_GET, sink)
            .await
            .1
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output.clone(),
            &["ID", "Title", "Kind", "Resource ID"],
            |item| {
                vec![
                    Cell::from(item.str("id")),
                    Cell::from(item.str("snippet.title")),
                    Cell::from(item.str("snippet.resourceId.kind")),
                    Cell::from(resource_id(item, "snippet.resourceId")),
                ]
            },
        );
        let err = self.pages(&svc, &mut sink).await;
        sink.finish(err)
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

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let mut q = self.clone();
        q.fields.parts = vec!["id".into(), "snippet".into(), "status".into()];
        // Divergence E18: the lookup fetches the one item; Go left the
        // count at 0, so it fetched nothing and every update failed.
        q.fields.max_results = 1;
        let (items, err) = q.get(ctx).await?;
        if let Some(e) = err {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            return Err(out);
        }
        let Some(mut item) = items.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        if !self.title.is_empty() {
            item.ensure("snippet").set_str("title", &self.title);
        }
        if let Some(d) = &self.description {
            item.ensure("snippet").set_str("description", d);
        }
        if !self.privacy.is_empty() {
            item.ensure("status")
                .set_str("privacyStatus", &self.privacy);
        }

        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.playlistItems.update")
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
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Playlist Item updated: {}\n", res.str("id")),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let mut call = svc.call("youtube.playlistItems.delete").param("id", id);
            if !self.fields.on_behalf_of_content_owner.is_empty() {
                call = call.param(
                    "onBehalfOfContentOwner",
                    &self.fields.on_behalf_of_content_owner,
                );
            }
            match call.send_empty().await {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Playlist Item {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
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
