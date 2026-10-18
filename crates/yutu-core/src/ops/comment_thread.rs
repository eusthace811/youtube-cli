//! Port of pkg/commentThread and cmd/commentThread.

use super::common::{Fields, cli_max_results};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::COMMENT_THREAD;

const ERR_GET: &str = "failed to get comment thread";
const ERR_INSERT: &str = "failed to insert comment thread";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CommentThread {
    #[serde(flatten)]
    pub fields: Fields,
    pub author_channel_id: String,
    pub moderation_status: String,
    pub order: String,
    pub search_terms: String,
    pub text_format: String,
    pub text_original: String,
    pub video_id: String,
    pub all_threads_related_to_channel_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
    Insert,
}

struct Bound(Verb, CommentThread);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut c = CommentThread::default();
    match verb {
        Verb::List => {
            c.fields.ids = if f.changed("ids") {
                f.strings("ids")
            } else {
                Vec::new()
            };
            c.all_threads_related_to_channel_id = f.string("allThreadsRelatedToChannelId");
            c.fields.channel_id = f.string("channelId");
            c.fields.max_results = cli_max_results(f.i64("maxResults"));
            c.moderation_status = f.string("moderationStatus");
            c.order = f.string("order");
            c.search_terms = f.string("searchTerms");
            c.text_format = f.string("textFormat");
            c.video_id = f.string("videoId");
            c.fields.parts = f.strings("parts");
            c.fields.output = f.string("output");
        }
        Verb::Insert => {
            c.author_channel_id = f.string("authorChannelId");
            c.fields.channel_id = f.string("channelId");
            c.text_original = f.string("textOriginal");
            c.video_id = f.string("videoId");
            c.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: CommentThread = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 2] = [
    Entry {
        path: "commentThread insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "commentThread list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        match self.0 {
            Verb::Insert => format!("Create comment thread on video {}", self.1.video_id),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let c = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => c.list(ctx).await,
                Verb::Insert => c.insert(ctx).await,
            }
        })
    }
}

impl CommentThread {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.commentThreads.list")
                .params("part", &self.fields.parts);
            if !self.fields.ids.is_empty() {
                c = c.params("id", &self.fields.ids);
            }
            for (name, value) in [
                (
                    "allThreadsRelatedToChannelId",
                    &self.all_threads_related_to_channel_id,
                ),
                ("channelId", &self.fields.channel_id),
                ("moderationStatus", &self.moderation_status),
                ("order", &self.order),
                ("searchTerms", &self.search_terms),
                ("textFormat", &self.text_format),
                ("videoId", &self.video_id),
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
            &["ID", "Author", "Video ID", "Text Display"],
            |t| {
                vec![
                    Cell::from(t.str("id")),
                    Cell::from(t.str("snippet.topLevelComment.snippet.authorDisplayName")),
                    Cell::from(t.str("snippet.topLevelComment.snippet.videoId")),
                    Cell::from(t.str("snippet.topLevelComment.snippet.textDisplay")),
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
        let mut ct = Value::new_struct(COMMENT_THREAD);
        ct.ensure("snippet")
            .set_str("channelId", &self.fields.channel_id);
        ct.ensure("snippet.topLevelComment.snippet")
            .set_str("channelId", &self.fields.channel_id)
            .set_str("textOriginal", &self.text_original)
            .set_str("videoId", &self.video_id);
        ct.ensure("snippet.topLevelComment.snippet.authorChannelId")
            .set_str("value", &self.author_channel_id);
        let res = svc
            .call("youtube.commentThreads.insert")
            .params("part", &["snippet".into()])
            .body(&ct)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("CommentThread inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }
}
