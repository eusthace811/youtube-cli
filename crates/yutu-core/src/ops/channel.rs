//! Port of pkg/channel and cmd/channel.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get channel";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Channel {
    #[serde(flatten)]
    pub fields: Fields,
    pub category_id: String,
    pub for_handle: String,
    pub for_username: String,
    #[serde(rename = "for")]
    pub for_: String,
    pub country: Option<String>,
    pub custom_url: String,
    pub default_language: Option<String>,
    pub description: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, Channel);

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
    }
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut c = Channel::default();
    match verb {
        Verb::List => {
            c.category_id = f.string("categoryId");
            c.for_handle = f.string("forHandle");
            c.for_username = f.string("forUsername");
            c.fields.hl = f.string("hl");
            c.fields.ids = ids(f, "ids");
            c.for_ = f.string("for");
            c.fields.max_results = cli_max_results(f.i64("maxResults"));
            c.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            c.fields.parts = f.strings("parts");
            c.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: Channel = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "channel list",
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

impl Channel {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.channels.list")
                .params("part", &self.fields.parts);
            for (name, value) in [
                ("categoryId", &self.category_id),
                ("forHandle", &self.for_handle),
                ("forUsername", &self.for_username),
                ("hl", &self.fields.hl),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            match self.for_.as_str() {
                "managedByMe" => c = c.param("managedByMe", true),
                "mine" => c = c.param("mine", true),
                "mySubscribers" => c = c.param("mySubscribers", true),
                _ => {}
            }
            if !self.fields.on_behalf_of_content_owner.is_empty() {
                c = c.param(
                    "onBehalfOfContentOwner",
                    &self.fields.on_behalf_of_content_owner,
                );
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
                &["ID", "Title", "Country"],
                |ch| {
                    vec![
                        Cell::from(ch.str("id")),
                        Cell::from(ch.str("snippet.title")),
                        Cell::from(ch.str("snippet.country")),
                    ]
                },
            )
        })
    }
}
