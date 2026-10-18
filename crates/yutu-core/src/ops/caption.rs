//! Port of pkg/caption and cmd/caption.

use super::common::{Fields, open_media};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list, print_result};
use serde::Deserialize;
use std::io::Write;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::CAPTION;

const ERR_GET: &str = "failed to get caption";
const ERR_UPDATE: &str = "failed to update caption";
const ERR_DELETE: &str = "failed to delete caption";
const ERR_INSERT: &str = "failed to insert caption";
const ERR_DOWNLOAD: &str = "failed to download caption";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Caption {
    #[serde(flatten)]
    pub fields: Fields,
    pub file: String,
    pub audio_track_type: String,
    pub is_auto_synced: Option<bool>,
    pub is_cc: Option<bool>,
    pub is_draft: Option<bool>,
    pub is_easy_reader: Option<bool>,
    pub is_large: Option<bool>,
    pub language: String,
    pub name: String,
    pub track_kind: String,
    pub on_behalf_of: String,
    pub video_id: String,
    pub tfmt: String,
    pub tlang: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Insert,
    Update,
    Delete,
    Download,
}

struct Bound(Verb, Caption);

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
    }
}

fn reset_bool(f: &dyn FlagValues, name: &str) -> Option<bool> {
    f.changed(name).then(|| f.bool(name))
}

/// A string `resetFlags` zeroes for the update command when it is unset.
fn reset_string(f: &dyn FlagValues, name: &str) -> String {
    if f.changed(name) {
        f.string(name)
    } else {
        String::new()
    }
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut c = Caption::default();
    match verb {
        Verb::List => {
            c.fields.ids = ids(f, "ids");
            c.video_id = f.string("videoId");
            c.on_behalf_of = f.string("onBehalfOf");
            c.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            c.fields.parts = f.strings("parts");
            c.fields.output = f.string("output");
        }
        Verb::Insert | Verb::Update => {
            c.file = f.string("file");
            if verb == Verb::Insert {
                c.audio_track_type = f.string("audioTrackType");
                c.track_kind = f.string("trackKind");
            } else {
                c.audio_track_type = reset_string(f, "audioTrackType");
                c.track_kind = reset_string(f, "trackKind");
            }
            c.is_auto_synced = reset_bool(f, "isAutoSynced");
            c.is_cc = reset_bool(f, "isCC");
            c.is_draft = reset_bool(f, "isDraft");
            c.is_easy_reader = reset_bool(f, "isEasyReader");
            c.is_large = reset_bool(f, "isLarge");
            c.language = f.string("language");
            c.name = f.string("name");
            c.video_id = f.string("videoId");
            c.on_behalf_of = f.string("onBehalfOf");
            c.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            c.fields.output = f.string("output");
        }
        Verb::Delete => {
            c.fields.ids = ids(f, "ids");
            c.on_behalf_of = f.string("onBehalfOf");
            c.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
        Verb::Download => {
            c.fields.ids = ids(f, "id");
            c.file = f.string("file");
            c.tfmt = f.string("tfmt");
            c.tlang = f.string("tlang");
            c.on_behalf_of = f.string("onBehalfOf");
            c.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: Caption = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 5] = [
    Entry {
        path: "caption delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "caption download",
        cli: |f| cli(Verb::Download, f),
        mcp: |a| mcp(Verb::Download, a),
    },
    Entry {
        path: "caption insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "caption list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "caption update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let c = &self.1;
        match self.0 {
            Verb::Delete => format!("Delete caption(s): {}", c.fields.ids.join(", ")),
            Verb::Insert => format!("Upload caption for video {} with {}", c.video_id, c.file),
            Verb::Update => {
                let mut target = c.video_id.clone();
                if !c.file.is_empty() {
                    target.push_str(" with file: ");
                    target.push_str(&c.file);
                }
                format!("Update caption for video {target}")
            }
            // Go does not gate download; divergence E1 does.
            Verb::Download => format!("Download caption {} to {}", c.first_id(), c.file),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let c = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => c.list(ctx).await,
                Verb::Insert => c.insert(ctx).await,
                Verb::Update => c.update(ctx).await,
                Verb::Delete => c.delete(ctx).await,
                Verb::Download => c.download(ctx).await,
            }
        })
    }
}

impl Caption {
    fn first_id(&self) -> &str {
        self.fields.ids.first().map_or("", String::as_str)
    }

    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<Vec<Value>, OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.captions.list")
            .params("part", &self.fields.parts)
            .param("videoId", &self.video_id);
        if !self.fields.ids.is_empty() {
            call = call.params("id", &self.fields.ids);
        }
        call = self.on_behalf(call);
        let res = call.send().await.map_err(|e| OpError::wrap(ERR_GET, e))?;
        Ok(res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default())
    }

    fn on_behalf<'s>(&self, mut call: yutu_api::Call<'s>) -> yutu_api::Call<'s> {
        if !self.on_behalf_of.is_empty() {
            call = call.param("onBehalfOf", &self.on_behalf_of);
        }
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        call
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let items = self.get(ctx).await?;
        print_list(
            &mut ctx.out,
            &self.fields.output,
            &items,
            &["ID", "Video ID", "Name", "Language"],
            |c| {
                vec![
                    Cell::from(c.str("id")),
                    Cell::from(c.str("snippet.videoId")),
                    Cell::from(c.str("snippet.name")),
                    Cell::from(c.str("snippet.language")),
                ]
            },
        )
        .map_err(OpError::new)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let mut caption = Value::new_struct(CAPTION);
        caption
            .ensure("snippet")
            .set_str("audioTrackType", &self.audio_track_type)
            .set(
                "isAutoSynced",
                Value::Bool(self.is_auto_synced == Some(true)),
            )
            .set("isCC", Value::Bool(self.is_cc == Some(true)))
            .set("isDraft", Value::Bool(self.is_draft == Some(true)))
            .set(
                "isEasyReader",
                Value::Bool(self.is_easy_reader == Some(true)),
            )
            .set("isLarge", Value::Bool(self.is_large == Some(true)))
            .set_str("language", &self.language)
            .set_str("name", &self.name)
            .set_str("trackKind", &self.track_kind)
            .set_str("videoId", &self.video_id);
        let call = svc
            .call("youtube.captions.insert")
            .params("part", &["snippet".into()])
            .body(&caption)
            .media(media);
        let res = self
            .on_behalf(call)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Caption inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let mut q = self.clone();
        q.fields.parts = vec!["snippet".into()];
        let captions = q.get(ctx).await.map_err(|e| {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            out
        })?;
        let Some(mut caption) = captions.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        {
            let s = caption.ensure("snippet");
            if !self.audio_track_type.is_empty() {
                s.set_str("audioTrackType", &self.audio_track_type);
            }
            for (json, go, value) in [
                ("isAutoSynced", "IsAutoSynced", self.is_auto_synced),
                ("isCC", "IsCC", self.is_cc),
                ("isDraft", "IsDraft", self.is_draft),
                ("isEasyReader", "IsEasyReader", self.is_easy_reader),
                ("isLarge", "IsLarge", self.is_large),
            ] {
                if let Some(v) = value {
                    s.set(json, Value::Bool(v)).force_send(go);
                }
            }
            for (json, value) in [
                ("language", &self.language),
                ("name", &self.name),
                ("trackKind", &self.track_kind),
                ("videoId", &self.video_id),
            ] {
                if !value.is_empty() {
                    s.set_str(json, value);
                }
            }
        }
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.captions.update")
            .params("part", &q.fields.parts)
            .body(&caption);
        if !self.file.is_empty() {
            let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
            call = call.media(media);
        }
        let res = self
            .on_behalf(call)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Caption updated: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let call = self.on_behalf(svc.call("youtube.captions.delete").param("id", id));
            match call.send_empty().await {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Caption {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    /// Streams the body into the file instead of buffering it as Go's
    /// io.ReadAll does; the bytes written are the same.
    async fn download(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let id = self.first_id();
        let mut call = svc.call("youtube.captions.download").param("id", id);
        if !self.tfmt.is_empty() {
            call = call.param("tfmt", &self.tfmt);
        }
        if !self.tlang.is_empty() {
            call = call.param("tlang", &self.tlang);
        }
        let mut res = self
            .on_behalf(call)
            .download()
            .await
            .map_err(|e| OpError::wrap(ERR_DOWNLOAD, e))?;
        let mut file =
            super::common::create_write(&self.file).map_err(|e| OpError::wrap(ERR_DOWNLOAD, e))?;
        while let Some(chunk) = res
            .chunk()
            .await
            .map_err(|e| OpError::wrap(ERR_DOWNLOAD, e))?
        {
            file.write_all(&chunk)
                .map_err(|e| OpError::wrap(ERR_DOWNLOAD, e))?;
        }
        let _ = ctx
            .out
            .write_str(&format!("Caption {id} downloaded to {}\n", self.file));
        Ok(())
    }
}
