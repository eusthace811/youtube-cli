//! Port of pkg/liveBroadcast and cmd/liveBroadcast.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list, print_result};
use serde::Deserialize;
use yutu_api::client::BoxFuture;
use yutu_api::generated::{CUEPOINT, LIVE_BROADCAST};
use yutu_api::{Call, Value};

const ERR_GET: &str = "failed to get live broadcast";
const ERR_INSERT: &str = "failed to insert live broadcast";
const ERR_UPDATE: &str = "failed to update live broadcast";
const ERR_DELETE: &str = "failed to delete live broadcast";
const ERR_BIND: &str = "failed to bind live broadcast";
const ERR_TRANSITION: &str = "failed to transition live broadcast";
const ERR_INSERT_CUEPOINT: &str = "failed to insert cuepoint";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LiveBroadcast {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub mine: Option<bool>,
    pub broadcast_status: String,
    pub broadcast_type: String,
    pub privacy_status: String,
    pub scheduled_start_time: String,
    pub scheduled_end_time: String,
    pub stream_id: String,
    pub cue_type: String,
    pub cue_duration_secs: i64,
    pub cue_insertion_offset_ms: i64,
    pub cue_walltime_ms: u64,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Insert,
    Update,
    Delete,
    Bind,
    Transition,
    InsertCuepoint,
}

struct Bound(Verb, LiveBroadcast);

/// The `parts` slice is shared by every subcommand of a Go package, so an
/// unset `--parts` holds the default of the last command registering it in
/// init order, not the invoked command's own default.
pub(super) fn shared_parts(f: &dyn FlagValues, last_default: &[&str]) -> Vec<String> {
    if f.changed("parts") {
        f.strings("parts")
    } else {
        last_default.iter().map(|p| p.to_string()).collect()
    }
}

const LAST_PARTS: &[&str] = &["id", "snippet", "status"];

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut b = LiveBroadcast::default();
    b.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
    b.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
    b.fields.output = f.string("output");
    match verb {
        Verb::List => {
            b.fields.ids = f.strings("ids");
            b.mine = f.changed("mine").then(|| f.bool("mine"));
            b.broadcast_status = f.string("broadcastStatus");
            b.broadcast_type = f.string("broadcastType");
            b.fields.max_results = cli_max_results(f.i64("maxResults"));
            b.fields.parts = shared_parts(f, LAST_PARTS);
        }
        Verb::Insert | Verb::Update => {
            if verb == Verb::Insert {
                b.fields.parts = shared_parts(f, LAST_PARTS);
            } else {
                b.fields.ids = f.strings("id");
                b.fields.max_results = 1;
            }
            b.title = f.string("title");
            b.description = f.changed("description").then(|| f.string("description"));
            b.scheduled_start_time = f.string("scheduledStartTime");
            b.scheduled_end_time = f.string("scheduledEndTime");
            b.privacy_status = f.string("privacyStatus");
        }
        Verb::Delete => b.fields.ids = f.strings("ids"),
        Verb::Bind => {
            b.fields.ids = f.strings("ids");
            b.stream_id = f.string("streamId");
            b.fields.parts = shared_parts(f, LAST_PARTS);
        }
        Verb::Transition => {
            b.fields.ids = f.strings("ids");
            b.broadcast_status = f.string("broadcastStatus");
            b.fields.parts = shared_parts(f, LAST_PARTS);
        }
        Verb::InsertCuepoint => {
            b.fields.ids = f.strings("ids");
            b.cue_type = f.string("cueType");
            b.cue_duration_secs = f.i64("cueDurationSecs");
            b.cue_insertion_offset_ms = f.i64("cueInsertionOffsetMs");
            b.cue_walltime_ms = f.u64("cueWalltimeMs");
        }
    }
    Box::new(Bound(verb, b))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let mut b: LiveBroadcast = serde_json::from_value(args).map_err(|e| e.to_string())?;
    if verb == Verb::Update {
        b.fields.max_results = 1;
    }
    Ok(Box::new(Bound(verb, b)))
}

pub static ENTRIES: [Entry; 7] = [
    Entry {
        path: "liveBroadcast bind",
        cli: |f| cli(Verb::Bind, f),
        mcp: |a| mcp(Verb::Bind, a),
    },
    Entry {
        path: "liveBroadcast delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "liveBroadcast insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "liveBroadcast insertCuepoint",
        cli: |f| cli(Verb::InsertCuepoint, f),
        mcp: |a| mcp(Verb::InsertCuepoint, a),
    },
    Entry {
        path: "liveBroadcast list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "liveBroadcast transition",
        cli: |f| cli(Verb::Transition, f),
        mcp: |a| mcp(Verb::Transition, a),
    },
    Entry {
        path: "liveBroadcast update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let b = &self.1;
        let ids = b.fields.ids.join(", ");
        match self.0 {
            Verb::List => String::new(),
            Verb::Insert => format!("Create live broadcast: {}", b.title),
            Verb::Update => format!("Update live broadcast: {ids}"),
            Verb::Delete => format!("Delete live broadcast(s): {ids}"),
            Verb::Bind => format!("Bind live broadcast(s) {ids} to stream {}", b.stream_id),
            Verb::Transition => {
                format!(
                    "Transition live broadcast(s) {ids} to {}",
                    b.broadcast_status
                )
            }
            Verb::InsertCuepoint => format!("Insert cuepoint into live broadcast(s) {ids}"),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let b = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => b.list(ctx).await,
                Verb::Insert => b.insert(ctx).await,
                Verb::Update => b.update(ctx).await,
                Verb::Delete => b.delete(ctx).await,
                Verb::Bind => b.bind(ctx).await,
                Verb::Transition => b.transition(ctx).await,
                Verb::InsertCuepoint => b.insert_cuepoint(ctx).await,
            }
        })
    }
}

impl LiveBroadcast {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.liveBroadcasts.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            if self.mine == Some(true) {
                c = c.param("mine", true);
            }
            for (name, value) in [
                ("broadcastStatus", &self.broadcast_status),
                ("broadcastType", &self.broadcast_type),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            self.owner_params(c)
        };
        Ok(paginate(self.fields.max_results, call, ERR_GET).await)
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
        let fetched = self.get(ctx).await?;
        finish_list(fetched, |items| {
            print_list(
                &mut ctx.out,
                &self.fields.output,
                items,
                &["ID", "Title", "Status", "Privacy"],
                |bc| {
                    vec![
                        Cell::from(bc.str("id")),
                        Cell::from(bc.str("snippet.title")),
                        Cell::from(bc.str("status.lifeCycleStatus")),
                        Cell::from(bc.str("status.privacyStatus")),
                    ]
                },
            )
        })
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut broadcast = Value::new_struct(LIVE_BROADCAST);
        {
            let s = broadcast.ensure("snippet");
            s.set_str("title", &self.title)
                .set_str("scheduledStartTime", &self.scheduled_start_time)
                .set_str("scheduledEndTime", &self.scheduled_end_time);
            if let Some(d) = &self.description {
                s.set_str("description", d);
            }
        }
        broadcast
            .ensure("status")
            .set_str("privacyStatus", &self.privacy_status);
        let call = svc
            .call("youtube.liveBroadcasts.insert")
            .params("part", &self.fields.parts)
            .body(&broadcast);
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
            &format!("Live broadcast inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let mut q = self.clone();
        q.fields.parts = vec!["id".into(), "snippet".into(), "status".into()];
        let (broadcasts, err) = q.get(ctx).await?;
        if let Some(e) = err {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            return Err(out);
        }
        let Some(original) = broadcasts.first() else {
            return Err(OpError::new(ERR_GET));
        };

        let pick = |mine: &str, path: &str| {
            if mine.is_empty() {
                original.str(path).to_string()
            } else {
                mine.to_string()
            }
        };
        let mut broadcast = Value::new_struct(LIVE_BROADCAST);
        broadcast
            .as_struct_mut()
            .unwrap()
            .set_str("id", original.str("id"));
        broadcast
            .ensure("snippet")
            .set_str("title", pick(&self.title, "snippet.title"))
            .set_str(
                "description",
                self.description
                    .clone()
                    .unwrap_or_else(|| original.str("snippet.description").to_string()),
            )
            .set_str(
                "scheduledStartTime",
                pick(&self.scheduled_start_time, "snippet.scheduledStartTime"),
            )
            .set_str(
                "scheduledEndTime",
                pick(&self.scheduled_end_time, "snippet.scheduledEndTime"),
            );
        broadcast.ensure("status").set_str(
            "privacyStatus",
            pick(&self.privacy_status, "status.privacyStatus"),
        );

        let svc = ctx.service().await?;
        let call = svc
            .call("youtube.liveBroadcasts.update")
            .params("part", &["snippet".into(), "status".into()])
            .body(&broadcast);
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
            &format!("Live broadcast updated: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let call = svc.call("youtube.liveBroadcasts.delete").param("id", id);
            match self.owner_params(call).send_empty().await {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Live broadcast {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    /// The per-id loop bind, transition and insertCuepoint share: a failed
    /// call joins the sentinel and cause and moves on to the next id.
    async fn each_id(
        &self,
        ctx: &mut Ctx<'_>,
        sentinel: &str,
        build: impl for<'s> Fn(Call<'s>, &str) -> Call<'s>,
        method: &str,
        line: impl Fn(&str, &Value) -> String,
    ) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let call = self.owner_params(build(svc.call(method), id));
            match call.send().await {
                Err(e) => {
                    errs.push(sentinel);
                    errs.push(e.to_string());
                }
                Ok(res) => {
                    if let Err(e) =
                        print_result(&mut ctx.out, &self.fields.output, &res, &line(id, &res))
                    {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    async fn bind(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        self.each_id(
            ctx,
            ERR_BIND,
            |c, id| {
                let c = c.param("id", id).params("part", &self.fields.parts);
                if self.stream_id.is_empty() {
                    c
                } else {
                    c.param("streamId", &self.stream_id)
                }
            },
            "youtube.liveBroadcasts.bind",
            |_, res| {
                format!(
                    "Live broadcast {} bound to stream {}\n",
                    res.str("id"),
                    self.stream_id
                )
            },
        )
        .await
    }

    async fn transition(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        self.each_id(
            ctx,
            ERR_TRANSITION,
            |c, id| {
                c.param("broadcastStatus", &self.broadcast_status)
                    .param("id", id)
                    .params("part", &self.fields.parts)
            },
            "youtube.liveBroadcasts.transition",
            |id, _| {
                format!(
                    "Live broadcast {id} transitioned to {}\n",
                    self.broadcast_status
                )
            },
        )
        .await
    }

    async fn insert_cuepoint(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let mut cuepoint = Value::new_struct(CUEPOINT);
        cuepoint
            .as_struct_mut()
            .unwrap()
            .set_str("cueType", &self.cue_type)
            .set("durationSecs", Value::Int(self.cue_duration_secs))
            .set(
                "insertionOffsetTimeMs",
                Value::Int(self.cue_insertion_offset_ms),
            )
            .set("walltimeMs", Value::Uint(self.cue_walltime_ms));
        self.each_id(
            ctx,
            ERR_INSERT_CUEPOINT,
            |c, id| c.body(&cuepoint).param("id", id),
            "youtube.liveBroadcasts.insertCuepoint",
            |id, res| format!("Cuepoint inserted for broadcast {id}: {}\n", res.str("id")),
        )
        .await
    }
}
