//! Port of pkg/comment and cmd/comment.

use super::common::{Fields, cli_max_results, finish_list, paginate};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, Out, print_list, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::COMMENT;
use yutu_api::json::{self, Json};

const ERR_GET: &str = "failed to get comment";
const ERR_MARK_AS_SPAM: &str = "failed to mark comment as spam";
const ERR_DELETE: &str = "failed to delete comment";
const ERR_INSERT: &str = "failed to insert comment";
const ERR_UPDATE: &str = "failed to update comment";
const ERR_SET_MODERATION_STATUS: &str = "failed to set comment moderation status";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Comment {
    #[serde(flatten)]
    pub fields: Fields,
    /// `common.Fields.Confirmed`, which markAsSpam and setModerationStatus
    /// echo back in their JSON output.
    pub confirmed: bool,
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
    Insert,
    Update,
    Delete,
    MarkAsSpam,
    SetModerationStatus,
}

struct Bound(Verb, Comment);

fn ids(f: &dyn FlagValues, name: &str) -> Vec<String> {
    if f.changed(name) {
        f.strings(name)
    } else {
        Vec::new()
    }
}

fn reset_bool(f: &dyn FlagValues, name: &str) -> Option<bool> {
    f.changed(name).then(|| f.bool(name))
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
        Verb::Insert => {
            c.author_channel_id = f.string("authorChannelId");
            c.fields.channel_id = f.string("channelId");
            c.can_rate = reset_bool(f, "canRate");
            c.parent_id = f.string("parentId");
            c.text_original = f.string("textOriginal");
            c.video_id = f.string("videoId");
            c.fields.output = f.string("output");
        }
        Verb::Update => {
            c.fields.ids = ids(f, "id");
            c.can_rate = reset_bool(f, "canRate");
            c.text_original = f.string("textOriginal");
            c.viewer_rating = f.string("viewerRating");
            c.fields.output = f.string("output");
        }
        Verb::Delete => c.fields.ids = ids(f, "ids"),
        Verb::MarkAsSpam => {
            c.fields.ids = ids(f, "ids");
            c.fields.output = f.string("output");
        }
        Verb::SetModerationStatus => {
            c.fields.ids = ids(f, "ids");
            c.moderation_status = f.string("moderationStatus");
            c.ban_author = reset_bool(f, "banAuthor");
            c.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, c))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let c: Comment = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, c)))
}

pub static ENTRIES: [Entry; 6] = [
    Entry {
        path: "comment delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "comment insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "comment list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "comment markAsSpam",
        cli: |f| cli(Verb::MarkAsSpam, f),
        mcp: |a| mcp(Verb::MarkAsSpam, a),
    },
    Entry {
        path: "comment setModerationStatus",
        cli: |f| cli(Verb::SetModerationStatus, f),
        mcp: |a| mcp(Verb::SetModerationStatus, a),
    },
    Entry {
        path: "comment update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let c = &self.1;
        let ids = c.fields.ids.join(", ");
        match self.0 {
            Verb::Delete => format!("Delete comment(s): {ids}"),
            Verb::Insert => format!("Create comment on parent {}", c.parent_id),
            Verb::Update => format!("Update comment: {ids}"),
            Verb::MarkAsSpam => format!("Mark comment(s) {ids} as spam"),
            Verb::SetModerationStatus => format!(
                "Set moderation status of comment(s) {ids} to {}",
                c.moderation_status
            ),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let c = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => c.list(ctx).await,
                Verb::Insert => c.insert(ctx).await,
                Verb::Update => c.update(ctx).await,
                Verb::Delete => c.delete(ctx).await,
                Verb::MarkAsSpam => c.mark_as_spam(ctx).await,
                Verb::SetModerationStatus => c.set_moderation_status(ctx).await,
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

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut comment = Value::new_struct(COMMENT);
        comment
            .ensure("snippet.authorChannelId")
            .set_str("value", &self.author_channel_id);
        {
            let s = comment.ensure("snippet");
            s.set_str("channelId", &self.fields.channel_id)
                .set_str("parentId", &self.parent_id)
                .set_str("textOriginal", &self.text_original)
                .set_str("videoId", &self.video_id);
            if let Some(r) = self.can_rate {
                s.set("canRate", Value::Bool(r));
            }
        }
        let res = svc
            .call("youtube.comments.insert")
            .params("part", &["snippet".into()])
            .body(&comment)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Comment inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        let mut q = self.clone();
        q.fields.parts = vec!["id".into(), "snippet".into()];
        let (comments, err) = q.get(ctx).await.map_err(|e| prefixed(ERR_UPDATE, e))?;
        if let Some(e) = err {
            return Err(prefixed(ERR_UPDATE, e));
        }
        let Some(mut comment) = comments.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        {
            let s = comment.ensure("snippet");
            if let Some(r) = self.can_rate {
                s.set("canRate", Value::Bool(r)).force_send("CanRate");
            }
            if !self.text_original.is_empty() {
                s.set_str("textOriginal", &self.text_original);
            }
            if !self.viewer_rating.is_empty() {
                s.set_str("viewerRating", &self.viewer_rating);
            }
        }
        let svc = ctx.service().await?;
        let res = svc
            .call("youtube.comments.update")
            .params("part", &["snippet".into()])
            .body(&comment)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Comment updated: {id}\n"),
        )
        .map_err(OpError::new)
    }

    async fn mark_as_spam(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        svc.call("youtube.comments.markAsSpam")
            .params("id", &self.fields.ids)
            .send_empty()
            .await
            .map_err(|e| OpError::wrap(ERR_MARK_AS_SPAM, e))?;
        self.print_self(
            &mut ctx.out,
            &format!("Comment marked as spam: {}\n", go_slice(&self.fields.ids)),
        )
        .map_err(OpError::new)
    }

    async fn set_moderation_status(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.comments.setModerationStatus")
            .params("id", &self.fields.ids)
            .param("moderationStatus", &self.moderation_status);
        if let Some(b) = self.ban_author {
            call = call.param("banAuthor", b);
        }
        call.send_empty()
            .await
            .map_err(|e| OpError::wrap(ERR_SET_MODERATION_STATUS, e))?;
        self.print_self(
            &mut ctx.out,
            &format!(
                "Comment moderation status set to {}: {}\n",
                self.moderation_status,
                go_slice(&self.fields.ids)
            ),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut errs = OpError::default();
        for id in &self.fields.ids {
            match svc
                .call("youtube.comments.delete")
                .param("id", id)
                .send_empty()
                .await
            {
                Err(e) => {
                    errs.push(ERR_DELETE);
                    errs.push(e.to_string());
                }
                Ok(()) => {
                    if let Err(e) = ctx.out.write_str(&format!("Comment {id} deleted\n")) {
                        errs.push(e);
                    }
                }
            }
        }
        errs.into_result()
    }

    /// `PrintResult` with the input struct itself as the data, as Go's
    /// markAsSpam and setModerationStatus do.
    fn print_self(&self, out: &mut Out<'_>, human: &str) -> Result<(), String> {
        match self.fields.output.as_str() {
            "json" => out.write_str(&format!(
                "{}\n",
                json::write(&self.to_json(), out.interactive)
            )),
            "yaml" => out.write_str(&format!("{}\n", self.to_yaml())),
            "silent" => Ok(()),
            _ => out.write_str(human),
        }
    }

    /// encoding/json/v2 of the Go struct: `omitempty` drops "" and empty
    /// slices, `omitzero` drops 0 and nil pointers, and false stays.
    fn to_json(&self) -> Json {
        let f = &self.fields;
        let mut obj: Vec<(String, Json)> = Vec::new();
        let strings = |v: &[String]| Json::Array(v.iter().cloned().map(Json::Str).collect());
        if !f.ids.is_empty() {
            obj.push(("ids".into(), strings(&f.ids)));
        }
        obj.push(("confirmed".into(), Json::Bool(self.confirmed)));
        if f.max_results != 0 {
            obj.push((
                "max_results".into(),
                Json::Number(f.max_results.to_string()),
            ));
        }
        let text = |obj: &mut Vec<(String, Json)>, k: &str, v: &str| {
            if !v.is_empty() {
                obj.push((k.into(), Json::Str(v.into())));
            }
        };
        text(&mut obj, "hl", &f.hl);
        text(&mut obj, "channel_id", &f.channel_id);
        if !f.parts.is_empty() {
            obj.push(("parts".into(), strings(&f.parts)));
        }
        text(&mut obj, "output", &f.output);
        text(
            &mut obj,
            "on_behalf_of_content_owner",
            &f.on_behalf_of_content_owner,
        );
        text(&mut obj, "author_channel_id", &self.author_channel_id);
        if let Some(b) = self.can_rate {
            obj.push(("can_rate".into(), Json::Bool(b)));
        }
        text(&mut obj, "parent_id", &self.parent_id);
        text(&mut obj, "text_format", &self.text_format);
        text(&mut obj, "text_original", &self.text_original);
        text(&mut obj, "moderation_status", &self.moderation_status);
        if let Some(b) = self.ban_author {
            obj.push(("ban_author".into(), Json::Bool(b)));
        }
        text(&mut obj, "video_id", &self.video_id);
        text(&mut obj, "viewer_rating", &self.viewer_rating);
        Json::Object(obj)
    }

    /// yaml.v3 of the Go struct: the embedded `common.Fields` has no inline
    /// tag, so it nests under `fields`, and every field is written.
    fn to_yaml(&self) -> String {
        let f = &self.fields;
        let opt = |b: Option<bool>| b.map_or("null".to_string(), |b| b.to_string());
        let mut out = String::from("fields:\n");
        for (k, v) in [
            ("ids", YamlField::Seq(&f.ids)),
            ("max_results", YamlField::Plain(f.max_results.to_string())),
            ("hl", YamlField::Str(&f.hl)),
            ("channel_id", YamlField::Str(&f.channel_id)),
            ("parts", YamlField::Seq(&f.parts)),
            ("output", YamlField::Str(&f.output)),
            (
                "on_behalf_of_content_owner",
                YamlField::Str(&f.on_behalf_of_content_owner),
            ),
        ] {
            v.write(k, 4, &mut out);
        }
        for (k, v) in [
            ("author_channel_id", YamlField::Str(&self.author_channel_id)),
            ("can_rate", YamlField::Plain(opt(self.can_rate))),
            ("parent_id", YamlField::Str(&self.parent_id)),
            ("text_format", YamlField::Str(&self.text_format)),
            ("text_original", YamlField::Str(&self.text_original)),
            ("moderation_status", YamlField::Str(&self.moderation_status)),
            ("ban_author", YamlField::Plain(opt(self.ban_author))),
            ("video_id", YamlField::Str(&self.video_id)),
            ("viewer_rating", YamlField::Str(&self.viewer_rating)),
        ] {
            v.write(k, 0, &mut out);
        }
        out
    }
}

enum YamlField<'a> {
    Plain(String),
    Str(&'a str),
    Seq(&'a [String]),
}

impl YamlField<'_> {
    /// Writes `key: value` at `indent`, letting yutu-api's yaml.v3 encoder
    /// choose each scalar's style and lay out sequences.
    fn write(&self, key: &str, indent: usize, out: &mut String) {
        let pad = " ".repeat(indent);
        let rendered = match self {
            YamlField::Plain(s) => format!("{s}\n"),
            YamlField::Str(s) => yutu_api::yaml::value(&Value::Str(s.to_string())),
            YamlField::Seq(v) => yutu_api::yaml::value(&Value::Strings(v.to_vec())),
        };
        let mut lines = rendered.lines();
        let first = lines.next().unwrap_or("");
        if first.starts_with("- ") {
            out.push_str(&format!("{pad}{key}:\n"));
            for line in rendered.lines() {
                out.push_str(&format!("{pad}    {line}\n"));
            }
            return;
        }
        out.push_str(&format!("{pad}{key}: {first}\n"));
        for line in lines {
            out.push_str(&format!("{pad}{line}\n"));
        }
    }
}

/// Go's `%s` of a `[]string`.
fn go_slice(v: &[String]) -> String {
    format!("[{}]", v.join(" "))
}

/// `errors.Join(sentinel, err)` where `err` may itself be a join.
fn prefixed(sentinel: &str, e: OpError) -> OpError {
    let mut out = OpError::new(sentinel);
    out.extend(e);
    out
}
