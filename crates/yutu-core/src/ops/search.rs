//! Port of pkg/search and cmd/search.

use super::common::{Fields, cli_max_results};
use super::playlist_item::resource_id;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::Cell;
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get search";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Search {
    #[serde(flatten)]
    pub fields: Fields,
    pub channel_type: String,
    pub event_type: String,
    pub r#for: String,
    pub location: String,
    pub location_radius: String,
    pub order: String,
    pub published_after: String,
    pub published_before: String,
    pub q: String,
    pub region_code: String,
    pub relevance_language: String,
    pub safe_search: String,
    pub topic_id: String,
    pub types: Vec<String>,
    pub video_caption: String,
    pub video_category_id: String,
    pub video_definition: String,
    pub video_dimension: String,
    pub video_duration: String,
    pub video_embeddable: String,
    pub video_license: String,
    pub video_paid_product_placement: String,
    pub video_syndicated: String,
    pub video_type: String,
}

impl Search {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.search.list")
                .params("part", &self.fields.parts);
            for (name, value) in [
                ("channelId", &self.fields.channel_id),
                ("channelType", &self.channel_type),
                ("eventType", &self.event_type),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            match self.r#for.as_str() {
                "mine" => c = c.param("forMine", true),
                "developer" => c = c.param("forDeveloper", true),
                "contentOwner" => c = c.param("forContentOwner", true),
                _ => {}
            }
            for (name, value) in [
                ("location", &self.location),
                ("locationRadius", &self.location_radius),
                (
                    "onBehalfOfContentOwner",
                    &self.fields.on_behalf_of_content_owner,
                ),
                ("order", &self.order),
                ("publishedAfter", &self.published_after),
                ("publishedBefore", &self.published_before),
                ("q", &self.q),
                ("regionCode", &self.region_code),
                ("relevanceLanguage", &self.relevance_language),
                ("safeSearch", &self.safe_search),
                ("topicId", &self.topic_id),
            ] {
                if !value.is_empty() {
                    c = c.param(name, value);
                }
            }
            if !self.types.is_empty() {
                c = c.params("type", &self.types);
            }
            for (name, value) in [
                ("videoCaption", &self.video_caption),
                ("videoCategoryId", &self.video_category_id),
                ("videoDefinition", &self.video_definition),
                ("videoDimension", &self.video_dimension),
                ("videoDuration", &self.video_duration),
                ("videoEmbeddable", &self.video_embeddable),
                ("videoLicense", &self.video_license),
                (
                    "videoPaidProductPlacement",
                    &self.video_paid_product_placement,
                ),
                ("videoSyndicated", &self.video_syndicated),
                ("videoType", &self.video_type),
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
            &["Kind", "Title", "Resource ID"],
            |r| {
                vec![
                    Cell::from(r.str("id.kind")),
                    Cell::from(r.str("snippet.title")),
                    Cell::from(resource_id(r, "id")),
                ]
            },
        );
        let err = super::common::paginate_to(self.fields.max_results, call, ERR_GET, &mut sink)
            .await
            .1;
        sink.finish(err)
    }
}

impl Invoke for Search {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(Search {
        fields: Fields {
            channel_id: f.string("channelId"),
            max_results: cli_max_results(f.i64("maxResults")),
            on_behalf_of_content_owner: f.string("onBehalfOfContentOwner"),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
        channel_type: f.string("channelType"),
        event_type: f.string("eventType"),
        r#for: f.string("for"),
        location: f.string("location"),
        location_radius: f.string("locationRadius"),
        order: f.string("order"),
        published_after: f.string("publishedAfter"),
        published_before: f.string("publishedBefore"),
        q: f.string("q"),
        region_code: f.string("regionCode"),
        relevance_language: f.string("relevanceLanguage"),
        safe_search: f.string("safeSearch"),
        topic_id: f.string("topicId"),
        types: f.strings("types"),
        video_caption: f.string("videoCaption"),
        video_category_id: f.string("videoCategoryId"),
        video_definition: f.string("videoDefinition"),
        video_dimension: f.string("videoDimension"),
        video_duration: f.string("videoDuration"),
        video_embeddable: f.string("videoEmbeddable"),
        video_license: f.string("videoLicense"),
        video_paid_product_placement: f.string("videoPaidProductPlacement"),
        video_syndicated: f.string("videoSyndicated"),
        video_type: f.string("videoType"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<Search>(args)
        .map(|s| Box::new(s) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "search list",
    cli,
    mcp,
}];
