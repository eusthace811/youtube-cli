//! Port of pkg/subscription and cmd/subscription.

use super::common::{Fields, cli_max_results};
use super::playlist_item::resource_id;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::SUBSCRIPTION;

const ERR_GET: &str = "failed to get subscription";
const ERR_DELETE: &str = "failed to delete subscription";
const ERR_INSERT: &str = "failed to insert subscription";

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
    Insert,
    Delete,
}

struct Bound(Verb, Subscription);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut s = Subscription::default();
    match verb {
        Verb::Insert => {
            s.subscriber_channel_id = f.string("subscriberChannelId");
            s.description = f.string("description");
            s.fields.channel_id = f.string("channelId");
            s.title = f.string("title");
            s.fields.output = f.string("output");
        }
        Verb::Delete => s.fields.ids = f.strings("ids"),
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

pub static ENTRIES: [Entry; 3] = [
    Entry {
        path: "subscription delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "subscription insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "subscription list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let s = &self.1;
        match self.0 {
            Verb::Delete => format!("Delete subscription(s): {}", s.fields.ids.join(", ")),
            Verb::Insert => format!("Subscribe to channel: {}", s.fields.channel_id),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let s = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => s.list(ctx).await,
                Verb::Insert => s.insert(ctx).await,
                Verb::Delete => s.delete(ctx).await,
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
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output,
            &["ID", "Kind", "Resource ID", "Channel Title"],
            |sub| {
                vec![
                    Cell::from(sub.str("id")),
                    Cell::from(sub.str("snippet.resourceId.kind")),
                    Cell::from(resource_id(sub, "snippet.resourceId")),
                    Cell::from(sub.str("snippet.title")),
                ]
            },
        );
        let err = super::common::paginate_to(self.fields.max_results, call, ERR_GET, &mut sink)
            .await
            .1;
        sink.finish(err)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut subscription = Value::new_struct(SUBSCRIPTION);
        subscription
            .ensure("snippet")
            .set_str("channelId", &self.subscriber_channel_id)
            .set_str("description", &self.description)
            .set_str("title", &self.title);
        subscription
            .ensure("snippet.resourceId")
            .set_str("channelId", &self.fields.channel_id);
        let res = svc
            .call("youtube.subscriptions.insert")
            .params("part", &["snippet".into()])
            .body(&subscription)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Subscription inserted: {}\n", res.str("id")),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.subscriptions.delete")
                .param("id", id)
                .send_empty()
                .await
            {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Subscription {id} deleted")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
