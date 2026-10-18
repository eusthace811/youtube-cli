//! Port of pkg/liveStream and cmd/liveStream.

use super::common::{Fields, cli_max_results};
use super::live_broadcast::shared_parts;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::client::BoxFuture;
use yutu_api::generated::LIVE_STREAM;
use yutu_api::{Call, Value};

const ERR_GET: &str = "failed to get live stream";
const ERR_INSERT: &str = "failed to insert live stream";
const ERR_UPDATE: &str = "failed to update live stream";
const ERR_DELETE: &str = "failed to delete live stream";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveStream {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub mine: Option<bool>,
    pub frame_rate: String,
    pub ingestion_type: String,
    pub resolution: String,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Insert,
    Update,
    Delete,
}

struct Bound(Verb, LiveStream);

const LAST_PARTS: &[&str] = &["id", "snippet", "cdn", "status"];

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut s = LiveStream::default();
    s.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
    s.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
    s.fields.output = f.string("output");
    match verb {
        Verb::List => {
            s.fields.ids = f.strings("ids");
            s.mine = f.changed("mine").then(|| f.bool("mine"));
            s.fields.max_results = cli_max_results(f.i64("maxResults"));
            s.fields.parts = shared_parts(f, LAST_PARTS);
        }
        Verb::Insert | Verb::Update => {
            if verb == Verb::Insert {
                s.fields.parts = shared_parts(f, LAST_PARTS);
            } else {
                s.fields.ids = f.strings("id");
                s.fields.max_results = 1;
            }
            s.title = f.string("title");
            s.description = f.changed("description").then(|| f.string("description"));
            s.frame_rate = f.string("frameRate");
            s.ingestion_type = f.string("ingestionType");
            s.resolution = f.string("resolution");
        }
        Verb::Delete => s.fields.ids = f.strings("ids"),
    }
    Box::new(Bound(verb, s))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let mut s: LiveStream = serde_json::from_value(args).map_err(|e| e.to_string())?;
    if verb == Verb::Update {
        s.fields.max_results = 1;
    }
    Ok(Box::new(Bound(verb, s)))
}

pub static ENTRIES: [Entry; 4] = [
    Entry {
        path: "liveStream delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "liveStream insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "liveStream list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "liveStream update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let s = &self.1;
        let ids = s.fields.ids.join(", ");
        match self.0 {
            Verb::List => String::new(),
            Verb::Insert => format!("Create live stream: {}", s.title),
            Verb::Update => format!("Update live stream: {ids}"),
            Verb::Delete => format!("Delete live stream(s): {ids}"),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let s = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => s.list(ctx).await,
                Verb::Insert => s.insert(ctx).await,
                Verb::Update => s.update(ctx).await,
                Verb::Delete => s.delete(ctx).await,
            }
        })
    }
}

impl LiveStream {
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
                .call("youtube.liveStreams.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            if self.mine == Some(true) {
                c = c.param("mine", true);
            }
            self.owner_params(c)
        };
        super::common::paginate_to(self.fields.max_results, call, ERR_GET, sink)
            .await
            .1
    }

    fn owner_params<'s>(&self, mut c: Call<'s>) -> Call<'s> {
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            c = c.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        if !self.on_behalf_of_content_owner_channel.is_empty() {
            c = c.param(
                "onBehalfOfContentOwnerChannel",
                &self.on_behalf_of_content_owner_channel,
            );
        }
        c
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut sink = super::common::ListSink::new(
            &mut ctx.out,
            &self.fields.output,
            &["ID", "Title", "Status"],
            |s| {
                vec![
                    Cell::from(s.str("id")),
                    Cell::from(s.str("snippet.title")),
                    Cell::from(s.str("status.streamStatus")),
                ]
            },
        );
        let err = self.pages(&svc, &mut sink).await;
        sink.finish(err)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut stream = Value::new_struct(LIVE_STREAM);
        {
            let sn = stream.ensure("snippet");
            sn.set_str("title", &self.title);
            if let Some(d) = &self.description {
                sn.set_str("description", d);
            }
        }
        stream
            .ensure("cdn")
            .set_str("frameRate", &self.frame_rate)
            .set_str("ingestionType", &self.ingestion_type)
            .set_str("resolution", &self.resolution);
        let call = svc
            .call("youtube.liveStreams.insert")
            .params("part", &self.fields.parts)
            .body(&stream);
        let res = self
            .owner_params(call)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Live stream inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let mut q = self.clone();
        q.fields.parts = vec!["id".into(), "snippet".into(), "cdn".into()];
        let (streams, err) = q.get(ctx).await?;
        if let Some(e) = err {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            return Err(out);
        }
        let Some(original) = streams.first() else {
            return Err(OpError::new(ERR_GET));
        };

        let pick = |mine: &str, path: &str| {
            if mine.is_empty() {
                original.str(path).to_string()
            } else {
                mine.to_string()
            }
        };
        let mut stream = Value::new_struct(LIVE_STREAM);
        stream
            .as_struct_mut()
            .unwrap()
            .set_str("id", original.str("id"));
        stream
            .ensure("snippet")
            .set_str("title", pick(&self.title, "snippet.title"))
            .set_str(
                "description",
                self.description
                    .clone()
                    .unwrap_or_else(|| original.str("snippet.description").to_string()),
            );
        stream
            .ensure("cdn")
            .set_str("frameRate", pick(&self.frame_rate, "cdn.frameRate"))
            .set_str(
                "ingestionType",
                pick(&self.ingestion_type, "cdn.ingestionType"),
            )
            .set_str("resolution", pick(&self.resolution, "cdn.resolution"));

        let svc = ctx.service().await?;
        let call = svc
            .call("youtube.liveStreams.update")
            .params("part", &["snippet".into(), "cdn".into()])
            .body(&stream);
        let res = self
            .owner_params(call)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Live stream updated: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let call = svc.call("youtube.liveStreams.delete").param("id", id);
            match self.owner_params(call).send_empty().await {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Live stream {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}
