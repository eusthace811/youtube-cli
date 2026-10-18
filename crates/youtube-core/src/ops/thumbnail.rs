//! Port of pkg/thumbnail and cmd/thumbnail.

use super::common::{Fields, open_media};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::print_result;
use serde::Deserialize;
use youtube_api::client::BoxFuture;

const ERR_SET: &str = "failed to set thumbnail";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Thumbnail {
    #[serde(flatten)]
    pub fields: Fields,
    pub file: String,
    pub video_id: String,
}

impl Thumbnail {
    pub async fn set(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_SET, e))?;
        let res = svc
            .call("youtube.thumbnails.set")
            .param("videoId", &self.video_id)
            .media(media)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_SET, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Thumbnail set for video {}", self.video_id),
        )
        .map_err(OpError::new)
    }
}

impl Invoke for Thumbnail {
    fn confirm(&self) -> String {
        format!("Set thumbnail {} for video {}", self.file, self.video_id)
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.set(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(Thumbnail {
        fields: Fields {
            output: f.string("output"),
            ..Fields::default()
        },
        file: f.string("file"),
        video_id: f.string("videoId"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<Thumbnail>(args)
        .map(|t| Box::new(t) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "thumbnail set",
    cli,
    mcp,
}];
