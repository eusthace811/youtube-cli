//! Port of pkg/playlist and cmd/playlist.

use super::common::{Fields, cli_max_results};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::PLAYLIST;

const ERR_GET: &str = "failed to get playlist";
const ERR_INSERT: &str = "failed to insert playlist";
const ERR_UPDATE: &str = "failed to update playlist";
const ERR_DELETE: &str = "failed to delete playlist";

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
    Insert,
    Update,
    Delete,
}

struct Bound(Verb, Playlist);

/// A `*bool` or `*string` flag that `resetFlags` nils out when the invoked
/// command has the flag and it was not set.
fn reset_bool(f: &dyn FlagValues, name: &str) -> Option<bool> {
    f.changed(name).then(|| f.bool(name))
}

fn reset_string(f: &dyn FlagValues, name: &str) -> Option<String> {
    f.changed(name).then(|| f.string(name))
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
        Verb::Insert => {
            p.title = f.string("title");
            p.description = reset_string(f, "description");
            p.tags = f.changed("tags").then(|| f.strings("tags"));
            p.language = reset_string(f, "language");
            p.fields.channel_id = f.string("channelId");
            p.privacy = f.string("privacy");
            p.fields.output = f.string("output");
        }
        Verb::Update => {
            p.fields.ids = f.strings("id");
            p.title = f.string("title");
            p.description = reset_string(f, "description");
            p.tags = f.changed("tags").then(|| f.strings("tags"));
            p.language = reset_string(f, "language");
            p.privacy = f.string("privacy");
            p.fields.output = f.string("output");
        }
        Verb::Delete => {
            p.fields.ids = f.strings("ids");
            p.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
    }
    Box::new(Bound(verb, p))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let p: Playlist = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, p)))
}

pub static ENTRIES: [Entry; 4] = [
    Entry {
        path: "playlist delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "playlist insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "playlist list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "playlist update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let p = &self.1;
        let ids = p.fields.ids.join(", ");
        match self.0 {
            Verb::Delete => format!("Delete playlist(s): {ids}"),
            Verb::Insert => format!("Create playlist: {}", p.title),
            Verb::Update => format!("Update playlist: {ids}"),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let p = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => p.list(ctx).await,
                Verb::Insert => p.insert(ctx).await,
                Verb::Update => p.update(ctx).await,
                Verb::Delete => p.delete(ctx).await,
            }
        })
    }
}

impl Playlist {
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
        super::common::paginate_to(self.fields.max_results, call, ERR_GET, sink)
            .await
            .1
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output.clone(),
            &["ID", "Channel ID", "Title"],
            |p| {
                vec![
                    Cell::from(p.str("id")),
                    Cell::from(p.str("snippet.channelId")),
                    Cell::from(p.str("snippet.title")),
                ]
            },
        );
        let err = self.pages(&svc, &mut sink).await;
        sink.finish(err)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut playlist = Value::new_struct(PLAYLIST);
        {
            let s = playlist.ensure("snippet");
            s.set_str("title", &self.title)
                .set_str("channelId", &self.fields.channel_id);
            if let Some(t) = &self.tags {
                s.set("tags", Value::Strings(t.clone()));
            }
            if let Some(d) = &self.description {
                s.set_str("description", d);
            }
            if let Some(l) = &self.language {
                s.set_str("defaultLanguage", l);
            }
        }
        playlist
            .ensure("status")
            .set_str("privacyStatus", &self.privacy);
        let mut call = svc
            .call("youtube.playlists.insert")
            .params("part", &["snippet".into(), "status".into()])
            .body(&playlist);
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
                call = call.param(name, value);
            }
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Playlist inserted: {}\n", res.str("id")),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let (playlists, err) = self.get(ctx).await?;
        if let Some(e) = err {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            return Err(out);
        }
        let Some(mut playlist) = playlists.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        if !self.title.is_empty() {
            playlist.ensure("snippet").set_str("title", &self.title);
        }
        if let Some(d) = &self.description {
            playlist.ensure("snippet").set_str("description", d);
        }
        if let Some(t) = &self.tags {
            playlist
                .ensure("snippet")
                .set("tags", Value::Strings(t.clone()));
        }
        if let Some(l) = &self.language {
            playlist.ensure("snippet").set_str("defaultLanguage", l);
        }
        if !self.privacy.is_empty() {
            playlist
                .ensure("status")
                .set_str("privacyStatus", &self.privacy);
        }

        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.playlists.update")
            .params("part", &["snippet".into(), "status".into()])
            .body(&playlist);
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
            &format!("Playlist updated: {}\n", res.str("id")),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let mut call = svc.call("youtube.playlists.delete").param("id", id);
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
                    if let Err(e) = ctx.out.write_str(&format!("Playlist {id} deleted")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
