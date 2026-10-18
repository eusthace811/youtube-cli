//! Port of pkg/subscription and cmd/subscription.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::playlist_item::resource_id;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get subscription";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Subscription {
    #[serde(flatten)]
    pub fields: Fields,
    pub subscriber_channel_id: String,
    pub description: String,
    pub for_channel_id: String,
    pub r#for: String,
    pub order: String,
    pub title: String,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, Subscription);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut s = Subscription::default();
    match verb {
        Verb::List => {
            s.fields.ids = f.strings("ids");
            s.fields.channel_id = f.string("channelId");
            s.for_channel_id = f.string("forChannelId");
            s.fields.max_results = cli_max_results(f.i64("maxResults"));
            s.r#for = f.string("for");
            s.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            s.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
            s.order = f.string("order");
            s.fields.parts = f.strings("parts");
            s.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, s))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let s: Subscription = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, s)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "subscription list",
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
        let s = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => s.list(ctx).await,
            }
        })
    }
}

impl Subscription {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.subscriptions.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            for (name, value) in [
                ("channelId", &self.fields.channel_id),
                ("forChannelId", &self.for_channel_id),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            match self.r#for.as_str() {
                "mine" => c = c.param("mine", true),
                "myRecentSubscribers" => c = c.param("myRecentSubscribers", true),
                "mySubscribers" => c = c.param("mySubscribers", true),
                _ => {}
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
                ("order", &self.order),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            c
        };
        let fetched = paginate(self.fields.max_results, call, ERR_GET).await;
        finish_list(fetched, |items| {
            print_list(
                &mut ctx.out,
                &self.fields.output,
                items,
                &["ID", "Kind", "Resource ID", "Channel Title"],
                |sub| {
                    vec![
                        Cell::from(sub.str("id")),
                        Cell::from(sub.str("snippet.resourceId.kind")),
                        Cell::from(resource_id(sub, "snippet.resourceId")),
                        Cell::from(sub.str("snippet.title")),
                    ]
                },
            )
        })
    }
}
