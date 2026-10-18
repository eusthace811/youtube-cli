//! Port of pkg/channelSection and cmd/channelSection.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use youtube_api::client::BoxFuture;

const ERR_GET: &str = "failed to get channel section";
const ERR_DELETE: &str = "failed to delete channel section";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ChannelSection {
    #[serde(flatten)]
    pub fields: Fields,
    pub mine: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Delete,
}

struct Bound(Verb, ChannelSection);

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
    }
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut cs = ChannelSection::default();
    match verb {
        Verb::List => {
            cs.fields.ids = ids(f, "ids");
            cs.fields.channel_id = f.string("channelId");
            cs.fields.hl = f.string("hl");
            cs.mine = f.changed("mine").then(|| f.bool("mine"));
            cs.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            cs.fields.parts = f.strings("parts");
            cs.fields.output = f.string("output");
        }
        Verb::Delete => {
            cs.fields.ids = ids(f, "ids");
            cs.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
    }
    Box::new(Bound(verb, cs))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let cs: ChannelSection = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, cs)))
}

pub static ENTRIES: [Entry; 2] = [
    Entry {
        path: "channelSection delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "channelSection list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        match self.0 {
            Verb::Delete => format!(
                "Delete channel section(s): {}",
                self.1.fields.ids.join(", ")
            ),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let cs = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => cs.list(ctx).await,
                Verb::Delete => cs.delete(ctx).await,
            }
        })
    }
}

impl ChannelSection {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.channelSections.list")
            .params("part", &self.fields.parts);
        if !self.fields.ids.is_empty() {
            call = call.params("id", &self.fields.ids);
        }
        if !self.fields.channel_id.is_empty() {
            call = call.param("channelId", &self.fields.channel_id);
        }
        if !self.fields.hl.is_empty() {
            call = call.param("hl", &self.fields.hl);
        }
        if let Some(m) = self.mine {
            call = call.param("mine", m);
        }
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        let res = call.send().await.map_err(|e| OpError::wrap(ERR_GET, e))?;
        let items = res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default();
        print_list(
            &mut ctx.out,
            &self.fields.output,
            &items,
            &["ID", "Channel ID", "Title"],
            |s| {
                vec![
                    Cell::from(s.str("id")),
                    Cell::from(s.str("snippet.channelId")),
                    Cell::from(s.str("snippet.title")),
                ]
            },
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let mut call = svc.call("youtube.channelSections.delete").param("id", id);
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
                    if let Err(e) = ctx
                        .out
                        .write_str(&format!("Channel section {id} deleted\n"))
                    {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
