//! Port of pkg/watermark and cmd/watermark.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use serde::Deserialize;
use yutu_api::client::BoxFuture;

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
    Unset,
}

struct Bound(Verb, Watermark);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut w = Watermark::default();
    w.fields.channel_id = f.string("channelId");
    match verb {
        Verb::Unset => {}
    }
    Box::new(Bound(verb, w))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let w: Watermark = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, w)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "watermark unset",
    cli: |f| cli(Verb::Unset, f),
    mcp: |a| mcp(Verb::Unset, a),
}];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let w = &self.1;
        match self.0 {
            Verb::Unset => format!("Unset watermark for channel {}", w.fields.channel_id),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let w = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::Unset => w.unset(ctx).await,
            }
        })
    }
}

impl Watermark {
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
