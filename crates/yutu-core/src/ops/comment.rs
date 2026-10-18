//! Port of pkg/comment and cmd/comment.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get comment";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Comment {
    #[serde(flatten)]
    pub fields: Fields,
    pub author_channel_id: String,
    pub can_rate: Option<bool>,
    pub parent_id: String,
    pub text_format: String,
    pub text_original: String,
    pub moderation_status: String,
    pub ban_author: Option<bool>,
    pub video_id: String,
    pub viewer_rating: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, Comment);

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
    }
}

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut c = Comment::default();
    match verb {
        Verb::List => {
            c.fields.ids = ids(f, "ids");
            c.fields.max_results = cli_max_results(f.i64("maxResults"));
            c.parent_id = f.string("parentId");
            c.text_format = f.string("textFormat");
            c.fields.parts = f.strings("parts");
            c.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: Comment = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "comment list",
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

impl Comment {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<(Vec<Value>, Option<OpError>), OpError> {
        let svc = ctx.service().await?;
        let call = || {
            let mut c = svc
                .call("youtube.comments.list")
                .params("part", &self.fields.parts);
            if self.fields.ids.first().is_some_and(|id| !id.is_empty()) {
                c = c.params("id", &self.fields.ids);
            }
            if !self.parent_id.is_empty() {
                c = c.param("parentId", &self.parent_id);
            }
            if !self.text_format.is_empty() {
                c = c.param("textFormat", &self.text_format);
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
                &["ID", "Author", "Video ID", "Text Display"],
                |cm| {
                    vec![
                        Cell::from(cm.str("id")),
                        Cell::from(cm.str("snippet.authorDisplayName")),
                        Cell::from(cm.str("snippet.videoId")),
                        Cell::from(cm.str("snippet.textDisplay")),
                    ]
                },
            )
        })
    }
}
