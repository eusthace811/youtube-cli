//! Port of pkg/playlistImage and cmd/playlistImage.

use super::common::{Fields, cli_max_results, open_media};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use youtube_api::Value;
use youtube_api::client::BoxFuture;
use youtube_api::generated::PLAYLIST_IMAGE;

const ERR_GET: &str = "failed to get playlist image";
const ERR_INSERT: &str = "failed to insert playlist image";
const ERR_UPDATE: &str = "failed to update playlist image";
const ERR_DELETE: &str = "failed to delete playlist image";

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
    Insert,
    Update,
    Delete,
}

struct Bound(Verb, PlaylistImage);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut pi = PlaylistImage::default();
    match verb {
        Verb::Insert | Verb::Update => {
            if verb == Verb::Insert {
                pi.file = f.string("file");
            } else {
                pi.fields.max_results = 1;
            }
            pi.playlist_id = f.string("playlistId");
            pi.r#type = f.string("type");
            pi.height = f.i64("height");
            pi.width = f.i64("width");
            pi.fields.output = f.string("output");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            pi.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
        }
        Verb::List => {
            pi.parent = f.string("parent");
            pi.fields.max_results = cli_max_results(f.i64("maxResults"));
            pi.fields.parts = f.strings("parts");
            pi.fields.output = f.string("output");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            pi.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
        }
        Verb::Delete => {
            pi.fields.ids = f.strings("ids");
            pi.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
    }
    Box::new(Bound(verb, pi))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let pi: PlaylistImage = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, pi)))
}

pub static ENTRIES: [Entry; 4] = [
    Entry {
        path: "playlistImage delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "playlistImage insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "playlistImage list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "playlistImage update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let pi = &self.1;
        match self.0 {
            Verb::Delete => format!("Delete playlist image(s): {}", pi.fields.ids.join(", ")),
            Verb::Insert => format!(
                "Upload playlist image {} for playlist {}",
                pi.file, pi.playlist_id
            ),
            Verb::Update => format!("Update playlist image: {}", pi.playlist_id),
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

impl PlaylistImage {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let mut all = super::common::Collect::default();
        let err = self.pages(&svc, &mut all).await;
        Ok((all.items, err))
    }

    async fn pages(
        &self,
        svc: &youtube_api::Service,
        sink: &mut dyn super::common::PageSink,
    ) -> Option<OpError> {
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
        super::common::paginate_to(self.fields.max_results, call, ERR_GET, sink)
            .await
            .1
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output.clone(),
            &["ID", "Kind", "Playlist ID", "Type"],
            |img| {
                vec![
                    Cell::from(img.str("id")),
                    Cell::from(img.str("kind")),
                    Cell::from(img.str("snippet.playlistId")),
                    Cell::from(img.str("snippet.type")),
                ]
            },
        );
        let err = self.pages(&svc, &mut sink).await;
        sink.finish(err)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let mut image = Value::new_struct(PLAYLIST_IMAGE);
        image
            .as_struct_mut()
            .unwrap()
            .set_str("kind", "youtube#playlistImages");
        image
            .ensure("snippet")
            .set_str("playlistId", &self.playlist_id)
            .set_str("type", &self.r#type)
            .set("height", Value::Int(self.height))
            .set("width", Value::Int(self.width));
        let mut call = svc.call("youtube.playlistImages.insert").body(&image);
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
            .media(media)
            .params("part", &["kind".into(), "snippet".into()])
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("PlaylistImage inserted: {}\n", res.str("id")),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let mut q = self.clone();
        q.fields.parts = vec!["id".into(), "kind".into(), "snippet".into()];
        let (images, err) = q.get(ctx).await?;
        if let Some(e) = err {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            return Err(out);
        }
        let Some(mut image) = images.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        {
            let s = image.ensure("snippet");
            if !self.playlist_id.is_empty() {
                s.set_str("playlistId", &self.playlist_id);
            }
            if !self.r#type.is_empty() {
                s.set_str("type", &self.r#type);
            }
            if self.height != 0 {
                s.set("height", Value::Int(self.height));
            }
            if self.width != 0 {
                s.set("width", Value::Int(self.width));
            }
        }

        let svc = ctx.service().await?;
        let mut call = svc.call("youtube.playlistImages.update").body(&image);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        if !self.file.is_empty() {
            let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
            call = call.media(media);
        }
        let res = call
            .params("part", &["id".into(), "kind".into(), "snippet".into()])
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("PlaylistImage updated: {}\n", res.str("id")),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let mut call = svc.call("youtube.playlistImages.delete").param("id", id);
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
                    if let Err(e) = ctx.out.write_str(&format!("PlaylistImage {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
