//! Port of pkg/watermark and cmd/watermark.

use super::common::{Fields, open_media};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::INVIDEO_BRANDING;

const ERR_SET: &str = "failed to set watermark";
const ERR_UNSET: &str = "failed to unset watermark";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Watermark {
    #[serde(flatten)]
    pub fields: Fields,
    pub file: String,
    pub in_video_position: String,
    pub duration_ms: u64,
    pub offset_ms: u64,
    pub offset_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    Set,
    Unset,
}

struct Bound(Verb, Watermark);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut w = Watermark::default();
    w.fields.channel_id = f.string("channelId");
    match verb {
        Verb::Set => {
            w.file = f.string("file");
            w.in_video_position = f.string("inVideoPosition");
            w.duration_ms = f.u64("durationMs");
            w.offset_ms = f.u64("offsetMs");
            w.offset_type = f.string("offsetType");
            w.fields.on_behalf_of_content_owner = f.string("onBehalfOfContentOwner");
        }
        Verb::Unset => {}
    }
    Box::new(Bound(verb, w))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let w: Watermark = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, w)))
}

pub static ENTRIES: [Entry; 2] = [
    Entry {
        path: "watermark set",
        cli: |f| cli(Verb::Set, f),
        mcp: |a| mcp(Verb::Set, a),
    },
    Entry {
        path: "watermark unset",
        cli: |f| cli(Verb::Unset, f),
        mcp: |a| mcp(Verb::Unset, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let w = &self.1;
        match self.0 {
            Verb::Set => format!(
                "Set watermark {} for channel {}",
                w.file, w.fields.channel_id
            ),
            Verb::Unset => format!("Unset watermark for channel {}", w.fields.channel_id),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let w = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::Set => w.set(ctx).await,
                Verb::Unset => w.unset(ctx).await,
            }
        })
    }
}

impl Watermark {
    async fn set(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_SET, e))?;
        let mut branding = Value::new_struct(INVIDEO_BRANDING);
        {
            let p = branding.ensure("position");
            if !self.in_video_position.is_empty() {
                p.set_str("type", "corner")
                    .set_str("cornerPosition", &self.in_video_position);
            }
        }
        {
            let t = branding.ensure("timing");
            if self.duration_ms != 0 {
                t.set("durationMs", Value::Uint(self.duration_ms));
            }
            if self.offset_ms != 0 {
                t.set("offsetMs", Value::Uint(self.offset_ms));
            }
            if !self.offset_type.is_empty() {
                t.set_str("type", &self.offset_type);
            }
        }
        let mut call = svc
            .call("youtube.watermarks.set")
            .param("channelId", &self.fields.channel_id)
            .body(&branding)
            .media(media);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        call.send_empty()
            .await
            .map_err(|e| OpError::wrap(ERR_SET, e))?;
        // Go discards the write error here.
        let _ = ctx.out.write_str(&format!(
            "Watermark set for channel {}\n",
            self.fields.channel_id
        ));
        Ok(())
    }

    async fn unset(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.watermarks.unset")
            .param("channelId", &self.fields.channel_id);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        call.send_empty()
            .await
            .map_err(|e| OpError::wrap(ERR_UNSET, e))?;
        // Go discards the write error here.
        let _ = ctx.out.write_str(&format!(
            "Watermark unset for channel {}\n",
            self.fields.channel_id
        ));
        Ok(())
    }
}
