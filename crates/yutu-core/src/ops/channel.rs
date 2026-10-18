//! Port of pkg/channel and cmd/channel.

use super::common::{Fields, cli_max_results};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get channel";
const ERR_UPDATE: &str = "failed to update channel";

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
    Update,
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
        Verb::Update => {
            c.fields.ids = ids(f, "id");
            c.country = f.changed("country").then(|| f.string("country"));
            c.custom_url = f.string("customUrl");
            c.default_language = f
                .changed("defaultLanguage")
                .then(|| f.string("defaultLanguage"));
            c.description = f.changed("description").then(|| f.string("description"));
            c.title = f.string("title");
            c.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: Channel = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 2] = [
    Entry {
        path: "channel list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "channel update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        match self.0 {
            Verb::Update => format!("Update channel: {}", self.1.fields.ids.join(", ")),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let c = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => c.list(ctx).await,
                Verb::Update => c.update(ctx).await,
            }
        })
    }
}

impl Channel {
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
        super::common::paginate_to(self.fields.max_results, call, ERR_GET, sink)
            .await
            .1
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output.clone(),
            &["ID", "Title", "Country"],
            |ch| {
                vec![
                    Cell::from(ch.str("id")),
                    Cell::from(ch.str("snippet.title")),
                    Cell::from(ch.str("snippet.country")),
                ]
            },
        );
        let err = self.pages(&svc, &mut sink).await;
        sink.finish(err)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let mut q = self.clone();
        q.fields.parts = vec!["snippet".into()];
        let (channels, err) = match q.get(ctx).await {
            Ok(fetched) => fetched,
            Err(e) => return Err(prefixed(ERR_UPDATE, e)),
        };
        if let Some(e) = err {
            return Err(prefixed(ERR_UPDATE, e));
        }
        let Some(mut cha) = channels.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        {
            let s = cha.ensure("snippet");
            if let Some(c) = &self.country {
                s.set_str("country", c);
            }
            if !self.custom_url.is_empty() {
                s.set_str("customUrl", &self.custom_url);
            }
            if let Some(l) = &self.default_language {
                s.set_str("defaultLanguage", l);
            }
            if let Some(d) = &self.description {
                s.set_str("description", d);
            }
            if !self.title.is_empty() {
                s.set_str("title", &self.title);
            }
        }
        let svc = ctx.service().await?;
        let res = svc
            .call("youtube.channels.update")
            .params("part", &q.fields.parts)
            .body(&cha)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Channel updated: {id}\n"),
        )
        .map_err(OpError::new)
    }
}

/// `errors.Join(sentinel, err)` where `err` may itself be a join.
fn prefixed(sentinel: &str, e: OpError) -> OpError {
    let mut out = OpError::new(sentinel);
    out.extend(e);
    out
}
