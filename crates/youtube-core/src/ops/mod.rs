//! Operations: one module per YouTube resource, each a port of the Go
//! pkg/<resource> code, reachable from the CLI and from MCP through
//! [`ENTRIES`].

pub mod abuse_report;
pub mod activity;
pub mod caption;
pub mod channel;
pub mod channel_banner;
pub mod channel_section;
pub mod comment;
pub mod comment_thread;
pub mod common;
pub mod i18n_language;
pub mod i18n_region;
pub mod live_broadcast;
pub mod live_chat_ban;
pub mod live_chat_message;
pub mod live_chat_moderator;
pub mod live_stream;
pub mod member;
pub mod memberships_level;
pub mod playlist;
pub mod playlist_image;
pub mod playlist_item;
pub mod search;
pub mod subscription;
pub mod super_chat_event;
pub mod third_party_link;
pub mod thumbnail;
pub mod video;
pub mod video_abuse_report_reason;
pub mod video_category;
pub mod watermark;

use crate::output::Out;
use serde_json::Value as Json;
use std::fmt;
use youtube_api::Service;
use youtube_api::client::BoxFuture;

/// Go's `errors.Join` result: each joined error on its own line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpError(pub Vec<String>);

impl OpError {
    pub fn new(msg: impl Into<String>) -> OpError {
        OpError(vec![msg.into()])
    }

    /// `errors.Join(sentinel, cause)`.
    pub fn wrap(sentinel: &str, cause: impl fmt::Display) -> OpError {
        OpError(vec![sentinel.to_string(), cause.to_string()])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, msg: impl Into<String>) {
        self.0.push(msg.into());
    }

    pub fn extend(&mut self, other: OpError) {
        self.0.extend(other.0);
    }

    /// `errors.Join(errs...)` with `nil` for an empty set.
    pub fn into_result(self) -> Result<(), OpError> {
        if self.0.is_empty() { Ok(()) } else { Err(self) }
    }
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl From<String> for OpError {
    fn from(s: String) -> OpError {
        OpError(vec![s])
    }
}

/// Parsed CLI flag values, as pflag exposes them to Go's RunE.
pub trait FlagValues {
    fn string(&self, name: &str) -> String;
    fn bool(&self, name: &str) -> bool;
    fn strings(&self, name: &str) -> Vec<String>;
    fn i64(&self, name: &str) -> i64;
    fn u64(&self, name: &str) -> u64;
    fn changed(&self, name: &str) -> bool;
}

impl FlagValues for crate::cli::Flags {
    fn string(&self, name: &str) -> String {
        crate::cli::Flags::string(self, name).to_string()
    }
    fn bool(&self, name: &str) -> bool {
        crate::cli::Flags::bool(self, name)
    }
    fn strings(&self, name: &str) -> Vec<String> {
        crate::cli::Flags::strings(self, name).to_vec()
    }
    fn i64(&self, name: &str) -> i64 {
        crate::cli::Flags::i64(self, name)
    }
    fn u64(&self, name: &str) -> u64 {
        crate::cli::Flags::u64(self, name)
    }
    fn changed(&self, name: &str) -> bool {
        crate::cli::Flags::changed(self, name)
    }
}

/// Builds the YouTube service the first time an op needs it, the way Go's
/// `EnsureService` calls `DefaultServiceProvider`.
pub trait ServiceProvider: Send + Sync {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>>;
}

pub struct Ctx<'a> {
    pub out: Out<'a>,
    provider: &'a dyn ServiceProvider,
    svc: Option<Service>,
}

impl<'a> Ctx<'a> {
    pub fn new(out: Out<'a>, provider: &'a dyn ServiceProvider) -> Ctx<'a> {
        Ctx {
            out,
            provider,
            svc: None,
        }
    }

    pub async fn service(&mut self) -> Result<Service, OpError> {
        if self.svc.is_none() {
            self.svc = Some(self.provider.service().await.map_err(OpError::new)?);
        }
        Ok(self.svc.clone().unwrap())
    }
}

/// A fully bound operation, ready to confirm and run.
pub trait Invoke: Send + Sync {
    /// The confirmation prompt Go shows for this op; empty when ungated.
    fn confirm(&self) -> String;
    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>>;
}

pub struct Entry {
    pub path: &'static str,
    pub cli: fn(&dyn FlagValues) -> Box<dyn Invoke>,
    pub mcp: fn(Json) -> Result<Box<dyn Invoke>, String>,
}

pub fn entries() -> impl Iterator<Item = &'static Entry> {
    [
        &abuse_report::ENTRIES[..],
        &activity::ENTRIES[..],
        &caption::ENTRIES[..],
        &channel::ENTRIES[..],
        &channel_banner::ENTRIES[..],
        &channel_section::ENTRIES[..],
        &comment::ENTRIES[..],
        &comment_thread::ENTRIES[..],
        &i18n_language::ENTRIES[..],
        &i18n_region::ENTRIES[..],
        &live_broadcast::ENTRIES[..],
        &live_chat_ban::ENTRIES[..],
        &live_chat_message::ENTRIES[..],
        &live_chat_moderator::ENTRIES[..],
        &live_stream::ENTRIES[..],
        &member::ENTRIES[..],
        &memberships_level::ENTRIES[..],
        &playlist::ENTRIES[..],
        &playlist_image::ENTRIES[..],
        &playlist_item::ENTRIES[..],
        &search::ENTRIES[..],
        &subscription::ENTRIES[..],
        &super_chat_event::ENTRIES[..],
        &third_party_link::ENTRIES[..],
        &thumbnail::ENTRIES[..],
        &video::ENTRIES[..],
        &video_abuse_report_reason::ENTRIES[..],
        &video_category::ENTRIES[..],
        &watermark::ENTRIES[..],
    ]
    .into_iter()
    .flatten()
}

/// thumbnail.Set with silent output, as video insert and update call it.
pub(crate) async fn thumbnail_set(
    ctx: &mut Ctx<'_>,
    video_id: &str,
    file: &str,
) -> Result<(), OpError> {
    let t = thumbnail::Thumbnail {
        fields: common::Fields {
            output: "silent".into(),
            ..Default::default()
        },
        file: file.into(),
        video_id: video_id.into(),
    };
    with_discarded_output(ctx, |ctx| Box::pin(async move { t.set(ctx).await })).await
}

/// playlistItem.Insert with silent output, as video insert and update call it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn playlist_item_insert(
    ctx: &mut Ctx<'_>,
    title: &str,
    description: Option<&str>,
    kind: &str,
    k_video_id: &str,
    playlist_id: &str,
    channel_id: &str,
    privacy: &str,
) -> Result<(), OpError> {
    let pi = playlist_item::PlaylistItem {
        fields: common::Fields {
            channel_id: channel_id.into(),
            output: "silent".into(),
            ..Default::default()
        },
        title: title.into(),
        description: description.map(str::to_string),
        kind: kind.into(),
        k_video_id: k_video_id.into(),
        playlist_id: playlist_id.into(),
        privacy: privacy.into(),
        ..Default::default()
    };
    with_discarded_output(ctx, |ctx| Box::pin(async move { pi.insert(ctx).await })).await
}

/// Runs a nested op writing to `io.Discard`, sharing the caller's service.
async fn with_discarded_output<F>(ctx: &mut Ctx<'_>, f: F) -> Result<(), OpError>
where
    F: for<'b, 'c> FnOnce(&'b mut Ctx<'c>) -> BoxFuture<'b, Result<(), OpError>>,
{
    let mut sink = std::io::sink();
    let svc = ctx.service().await?;
    let provider = Fixed(svc);
    let mut inner = Ctx::new(
        Out {
            w: &mut sink,
            interactive: false,
        },
        &provider,
    );
    f(&mut inner).await
}

struct Fixed(Service);

impl ServiceProvider for Fixed {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        let s = self.0.clone();
        Box::pin(async move { Ok(s) })
    }
}

pub fn find(path: &str) -> Option<&'static Entry> {
    entries().find(|e| e.path == path)
}
