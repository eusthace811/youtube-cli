//! Port of pkg/caption and cmd/caption.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get caption";

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
}

struct Bound(Verb, Caption);

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
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
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: Caption = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "caption list",
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
        let c = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => c.list(ctx).await,
            }
        })
    }
}

impl Caption {
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
}
