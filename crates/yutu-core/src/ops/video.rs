//! Port of pkg/video and cmd/video.

use super::common::{Fields, cli_max_results, open_media};
use super::{Ctx, Entry, FlagValues, Invoke, OpError, playlist_item_insert, thumbnail_set};
use crate::output::{Cell, print_list, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::{VIDEO, VIDEO_ABUSE_REPORT};

const ERR_GET: &str = "failed to get video";
const ERR_INSERT: &str = "failed to insert video";
const ERR_UPDATE: &str = "failed to update video";
const ERR_SET_THUMBNAIL: &str = "failed to set video thumbnail";
const ERR_INSERT_PLAYLIST_ITEM: &str = "failed to add video to playlist";
const ERR_RATING: &str = "failed to rate video";
const ERR_GET_RATING: &str = "failed to get rating";
const ERR_DELETE: &str = "failed to delete video";
const ERR_REPORT_ABUSE: &str = "failed to report abuse";
const ERR_SCHEDULE_NON_PRIVATE: &str = "publishAt requires privacy private; a public or unlisted video would go offline until the scheduled time";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Video {
    #[serde(flatten)]
    pub fields: Fields,
    pub auto_levels: Option<bool>,
    pub file: String,
    pub title: String,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub language: Option<String>,
    pub locale: String,
    pub license: String,
    pub thumbnail: String,
    pub rating: String,
    pub chart: String,
    pub comments: String,
    pub playlist_id: String,
    pub category_id: String,
    pub privacy: String,
    pub for_kids: Option<bool>,
    pub embeddable: Option<bool>,
    pub publish_at: Option<String>,
    pub region_code: String,
    pub reason_id: String,
    pub stabilize: Option<bool>,
    pub max_height: i64,
    pub max_width: i64,
    pub recording_date: Option<String>,
    pub contains_synthetic_media: Option<bool>,
    pub secondary_reason_id: String,
    pub notify_subscribers: Option<bool>,
    pub public_stats_viewable: Option<bool>,
    pub on_behalf_of_content_owner_channel: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Insert,
    Update,
    Rate,
    GetRating,
    Delete,
    ReportAbuse,
}

struct Bound(Verb, Video);

/// A `*bool` or `*string` flag that `resetFlags` nils out when the invoked
/// command has the flag and it was not set.
fn reset_bool(f: &dyn FlagValues, name: &str) -> Option<bool> {
    f.changed(name).then(|| f.bool(name))
}

fn reset_string(f: &dyn FlagValues, name: &str) -> Option<String> {
    f.changed(name).then(|| f.string(name))
}

/// A string variable shared across subcommands: unchanged, it keeps the
/// default of its last registration in Go's init order, which is "" for
/// every shared string in this package.
fn shared(f: &dyn FlagValues, name: &str) -> String {
    if f.changed(name) {
        f.string(name)
    } else {
        String::new()
    }
}

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
    }
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut v = Video::default();
    match verb {
        Verb::List => {
            v.fields.ids = ids(f, "ids");
            v.chart = f.string("chart");
            v.fields.hl = f.string("hl");
            v.locale = f.string("locale");
            v.category_id = f.string("videoCategoryId");
            v.region_code = f.string("regionCode");
            v.max_height = f.i64("maxHeight");
            v.max_width = f.i64("maxWidth");
            v.fields.max_results = cli_max_results(f.i64("maxResults"));
            v.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            v.rating = shared(f, "myRating");
            v.fields.parts = f.strings("parts");
            v.fields.output = f.string("output");
        }
        Verb::Insert => {
            v.auto_levels = reset_bool(f, "autoLevels");
            v.file = f.string("file");
            v.title = f.string("title");
            v.description = reset_string(f, "description");
            // The update command registers `tags` after insert with an empty
            // default, so an unset --tags here is an empty, non-nil slice.
            v.tags = Some(if f.changed("tags") {
                f.strings("tags")
            } else {
                Vec::new()
            });
            v.language = reset_string(f, "language");
            v.license = f.string("license");
            v.thumbnail = f.string("thumbnail");
            v.fields.channel_id = f.string("channelId");
            v.playlist_id = f.string("playlistId");
            v.category_id = f.string("categoryId");
            v.privacy = f.string("privacy");
            v.for_kids = Some(f.bool("forKids"));
            v.embeddable = reset_bool(f, "embeddable");
            v.contains_synthetic_media = Some(f.bool("containsSyntheticMedia"));
            v.recording_date = reset_string(f, "recordingDate");
            v.publish_at = reset_string(f, "publishAt");
            v.stabilize = reset_bool(f, "stabilize");
            v.notify_subscribers = reset_bool(f, "notifySubscribers");
            v.public_stats_viewable = reset_bool(f, "publicStatsViewable");
            v.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            v.on_behalf_of_content_owner_channel = f.string("onBehalfOfContentOwnerChannel");
            v.fields.output = f.string("output");
        }
        Verb::Update => {
            v.fields.ids = ids(f, "id");
            v.title = f.string("title");
            v.description = reset_string(f, "description");
            v.tags = f.changed("tags").then(|| f.strings("tags"));
            v.language = reset_string(f, "language");
            v.license = f.string("license");
            v.playlist_id = f.string("playlistId");
            v.thumbnail = f.string("thumbnail");
            v.category_id = f.string("categoryId");
            v.privacy = f.string("privacy");
            v.publish_at = reset_string(f, "publishAt");
            v.embeddable = reset_bool(f, "embeddable");
            v.contains_synthetic_media = reset_bool(f, "containsSyntheticMedia");
            v.recording_date = reset_string(f, "recordingDate");
            v.fields.max_results = 1;
            v.fields.output = f.string("output");
        }
        Verb::Rate => {
            v.fields.ids = ids(f, "ids");
            v.rating = f.string("rating");
        }
        Verb::GetRating => {
            v.fields.ids = ids(f, "ids");
            v.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
            v.fields.output = f.string("output");
        }
        Verb::Delete => v.fields.ids = ids(f, "ids"),
        Verb::ReportAbuse => {
            v.fields.ids = ids(f, "ids");
            v.reason_id = f.string("reasonId");
            v.secondary_reason_id = f.string("secondaryReasonId");
            v.comments = f.string("comments");
            v.language = reset_string(f, "language");
            v.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
    }
    Box::new(Bound(verb, v))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let mut v: Video = serde_json::from_value(args).map_err(|e| e.to_string())?;
    if verb == Verb::Update {
        v.fields.max_results = 1;
    }
    Ok(Box::new(Bound(verb, v)))
}

pub static ENTRIES: [Entry; 7] = [
    Entry {
        path: "video delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "video getRating",
        cli: |f| cli(Verb::GetRating, f),
        mcp: |a| mcp(Verb::GetRating, a),
    },
    Entry {
        path: "video insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "video list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "video rate",
        cli: |f| cli(Verb::Rate, f),
        mcp: |a| mcp(Verb::Rate, a),
    },
    Entry {
        path: "video reportAbuse",
        cli: |f| cli(Verb::ReportAbuse, f),
        mcp: |a| mcp(Verb::ReportAbuse, a),
    },
    Entry {
        path: "video update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let v = &self.1;
        let ids = v.fields.ids.join(", ");
        match self.0 {
            Verb::Delete => format!("Delete video(s): {ids}"),
            Verb::Insert => format!("Upload video: {}", v.file),
            Verb::Update => format!("Update video: {ids}"),
            Verb::Rate => format!("Rate video(s) {ids} as {}", v.rating),
            Verb::ReportAbuse => format!("Report abuse on video(s) {ids}"),
            Verb::List | Verb::GetRating => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let v = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => v.list(ctx).await,
                Verb::Insert => v.insert(ctx).await,
                Verb::Update => v.update(ctx).await,
                Verb::Rate => v.rate(ctx).await,
                Verb::GetRating => v.get_rating(ctx).await,
                Verb::Delete => v.delete(ctx).await,
                Verb::ReportAbuse => v.report_abuse(ctx).await,
            }
        })
    }
}

impl Video {
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
                .call("youtube.videos.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            for (name, value) in [
                ("chart", &self.chart),
                ("hl", &self.fields.hl),
                ("locale", &self.locale),
                ("videoCategoryId", &self.category_id),
                ("myRating", &self.rating),
                ("regionCode", &self.region_code),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            if self.max_height != 0 {
                c = c.param("maxHeight", self.max_height);
            }
            if self.max_width != 0 {
                c = c.param("maxWidth", self.max_width);
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
            &["ID", "Title", "Channel ID", "Views"],
            |v| {
                vec![
                    Cell::from(v.str("id")),
                    Cell::from(v.str("snippet.title")),
                    Cell::from(v.str("snippet.channelId")),
                    Cell::Uint(match v.at("statistics.viewCount") {
                        Some(Value::Uint(n)) => *n,
                        _ => 0,
                    }),
                ]
            },
        );
        let err = self.pages(&svc, &mut sink).await;
        sink.finish(err)
    }

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let tags = self
            .tags
            .clone()
            .unwrap_or_else(|| vec!["yutu🐰".to_string()]);
        let title = if self.title.is_empty() {
            file_stem(&self.file)
        } else {
            self.title.clone()
        };

        let mut video = Value::new_struct(VIDEO);
        {
            let s = video.ensure("snippet");
            s.set_str("title", &title).set("tags", Value::Strings(tags));
            s.set_str("categoryId", &self.category_id)
                .set_str("channelId", &self.fields.channel_id);
            if let Some(d) = &self.description {
                s.set_str("description", d);
            }
            if let Some(l) = &self.language {
                s.set_str("defaultLanguage", l)
                    .set_str("defaultAudioLanguage", l);
            }
        }
        {
            let st = video.ensure("status");
            st.set_str("license", &self.license)
                .set_str("privacyStatus", &self.privacy);
            st.force_send("SelfDeclaredMadeForKids")
                .force_send("ContainsSyntheticMedia");
            if let Some(p) = &self.publish_at {
                st.set_str("publishAt", p);
            }
            if let Some(e) = self.embeddable {
                st.set("embeddable", Value::Bool(e))
                    .force_send("Embeddable");
            }
            if let Some(k) = self.for_kids {
                st.set("selfDeclaredMadeForKids", Value::Bool(k));
            }
            if let Some(c) = self.contains_synthetic_media {
                st.set("containsSyntheticMedia", Value::Bool(c));
            }
            if let Some(p) = self.public_stats_viewable {
                st.set("publicStatsViewable", Value::Bool(p))
                    .force_send("PublicStatsViewable");
            }
        }
        let mut parts = "snippet,status".to_string();
        if let Some(r) = self.recording_date.as_deref().filter(|r| !r.is_empty()) {
            video.ensure("recordingDetails").set_str("recordingDate", r);
            parts.push_str(",recordingDetails");
        }

        let mut call = svc
            .call("youtube.videos.insert")
            .param("part", &parts)
            .body(&video);
        if let Some(a) = self.auto_levels {
            call = call.param("autoLevels", a);
        }
        if let Some(n) = self.notify_subscribers {
            call = call.param("notifySubscribers", n);
        }
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        if !self.on_behalf_of_content_owner_channel.is_empty() {
            call = call.param(
                "onBehalfOfContentOwnerChannel",
                &self.on_behalf_of_content_owner_channel,
            );
        }
        if let Some(s) = self.stabilize {
            call = call.param("stabilize", s);
        }
        let res = call
            .media(media)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let mut errs = self.follow_ups(ctx, &res, ERR_INSERT).await;
        let id = res.str("id").to_string();
        if let Err(e) = print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Video inserted: {id}\n"),
        ) {
            errs.push(e);
        }
        errs.into_result()
    }

    /// The thumbnail and playlist side steps shared by insert and update.
    async fn follow_ups(&self, ctx: &mut Ctx<'_>, res: &Value, sentinel: &str) -> OpError {
        let mut errs = OpError::default();
        if !self.thumbnail.is_empty()
            && let Err(e) = thumbnail_set(ctx, res.str("id"), &self.thumbnail).await
        {
            errs.push(sentinel);
            errs.push(ERR_SET_THUMBNAIL);
            errs.extend(e);
        }
        if !self.playlist_id.is_empty()
            && let Err(e) = playlist_item_insert(
                ctx,
                res.str("snippet.title"),
                Some(res.str("snippet.description")),
                "video",
                res.str("id"),
                &self.playlist_id,
                res.str("snippet.channelId"),
                res.str("status.privacyStatus"),
            )
            .await
        {
            errs.push(sentinel);
            errs.push(ERR_INSERT_PLAYLIST_ITEM);
            errs.extend(e);
        }
        errs
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let mut q = self.clone();
        q.fields.parts = vec!["id".into(), "snippet".into(), "status".into()];
        let (videos, err) = q.get(ctx).await?;
        if let Some(e) = err {
            let mut out = OpError::new(ERR_UPDATE);
            out.extend(e);
            return Err(out);
        }
        let Some(original) = videos.first() else {
            return Err(OpError::new(ERR_GET));
        };

        let mut video = Value::new_struct(VIDEO);
        video
            .as_struct_mut()
            .unwrap()
            .set_str("id", original.str("id"));
        video.ensure("snippet");
        video
            .ensure("status")
            .force_send("Embeddable")
            .force_send("PublicStatsViewable")
            .force_send("SelfDeclaredMadeForKids")
            .force_send("ContainsSyntheticMedia");
        if let Some(orig) = original.at("snippet").and_then(Value::as_struct) {
            let s = video.ensure("snippet");
            for k in [
                "title",
                "description",
                "tags",
                "categoryId",
                "defaultLanguage",
            ] {
                s.set(k, orig.get(k).unwrap().clone());
            }
        }
        if let Some(orig) = original.at("status").and_then(Value::as_struct) {
            let st = video.ensure("status");
            for k in [
                "embeddable",
                "license",
                "privacyStatus",
                "publicStatsViewable",
                "publishAt",
                "selfDeclaredMadeForKids",
                "containsSyntheticMedia",
            ] {
                st.set(k, orig.get(k).unwrap().clone());
            }
        }
        if !self.title.is_empty() {
            video.ensure("snippet").set_str("title", &self.title);
        }
        if let Some(d) = &self.description {
            video.ensure("snippet").set_str("description", d);
        }
        if let Some(t) = &self.tags {
            video
                .ensure("snippet")
                .set("tags", Value::Strings(t.clone()));
        }
        if let Some(l) = &self.language {
            video.ensure("snippet").set_str("defaultLanguage", l);
        }
        if !self.license.is_empty() {
            video.ensure("status").set_str("license", &self.license);
        }
        if !self.category_id.is_empty() {
            video
                .ensure("snippet")
                .set_str("categoryId", &self.category_id);
        }
        if !self.privacy.is_empty() {
            video
                .ensure("status")
                .set_str("privacyStatus", &self.privacy);
        }
        if let Some(p) = &self.publish_at {
            if !p.is_empty() && video.str("status.privacyStatus") != "private" {
                return Err(OpError(vec![
                    ERR_UPDATE.into(),
                    ERR_SCHEDULE_NON_PRIVATE.into(),
                ]));
            }
            video.ensure("status").set_str("publishAt", p);
        }
        if let Some(e) = self.embeddable {
            video.ensure("status").set("embeddable", Value::Bool(e));
        }
        if let Some(c) = self.contains_synthetic_media {
            video
                .ensure("status")
                .set("containsSyntheticMedia", Value::Bool(c));
        }
        let mut parts = "snippet,status".to_string();
        if let Some(r) = &self.recording_date {
            video.ensure("recordingDetails").set_str("recordingDate", r);
            parts.push_str(",recordingDetails");
        }

        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.videos.update")
            .param("part", &parts)
            .body(&video);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        let mut errs = self.follow_ups(ctx, &res, ERR_UPDATE).await;
        let id = res.str("id").to_string();
        if let Err(e) = print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Video updated: {id}\n"),
        ) {
            errs.push(e);
        }
        errs.into_result()
    }

    async fn rate(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.videos.rate")
                .param("id", id)
                .param("rating", &self.rating)
                .send_empty()
                .await
            {
                Err(e) => {
                    errs.push(ERR_RATING);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx
                        .out
                        .write_str(&format!("Video {id} rated {}\n", self.rating))
                    {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    async fn get_rating(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.videos.getRating")
            .params("id", &self.fields.ids);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_GET_RATING, e))?;
        let items = res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default();
        print_list(
            &mut ctx.out,
            &self.fields.output,
            &items,
            &["ID", "Rating"],
            |r| vec![Cell::from(r.str("videoId")), Cell::from(r.str("rating"))],
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let mut call = svc.call("youtube.videos.delete").param("id", id);
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
                    if let Err(e) = ctx.out.write_str(&format!("Video {id} deleted")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    async fn report_abuse(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            let mut report = Value::new_struct(VIDEO_ABUSE_REPORT);
            {
                let r = report.as_struct_mut().unwrap();
                r.set_str("comments", &self.comments)
                    .set_str("reasonId", &self.reason_id)
                    .set_str("secondaryReasonId", &self.secondary_reason_id)
                    .set_str("videoId", id);
                if let Some(l) = &self.language {
                    r.set_str("language", l);
                }
            }
            let mut call = svc.call("youtube.videos.reportAbuse").body(&report);
            if !self.fields.on_behalf_of_content_owner.is_empty() {
                call = call.param(
                    "onBehalfOfContentOwner",
                    &self.fields.on_behalf_of_content_owner,
                );
            }
            match call.send_empty().await {
                Err(e) => {
                    errs.push(ERR_REPORT_ABUSE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Video {id} reported for abuse")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }
}

/// `utils.GetFileName`: the base name without its extension.
pub fn file_stem(path: &str) -> String {
    let base = path
        .rsplit(['/', std::path::MAIN_SEPARATOR])
        .next()
        .unwrap_or(path);
    match base.rfind('.') {
        Some(i) => base[..i].to_string(),
        None => base.to_string(),
    }
}
