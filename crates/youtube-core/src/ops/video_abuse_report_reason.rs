//! Port of pkg/videoAbuseReportReason and cmd/videoAbuseReportReason.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use youtube_api::client::BoxFuture;

const ERR_GET: &str = "failed to get video abuse report reason";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VideoAbuseReportReason {
    #[serde(flatten)]
    pub fields: Fields,
}

impl VideoAbuseReportReason {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.videoAbuseReportReasons.list")
            .params("part", &self.fields.parts);
        if !self.fields.hl.is_empty() {
            call = call.param("hl", &self.fields.hl);
        }
        let res = call.send().await.map_err(|e| OpError::wrap(ERR_GET, e))?;
        let items = res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default();
        print_list(
            &mut ctx.out,
            &self.fields.output,
            &items,
            &["ID", "Label"],
            |r| vec![Cell::from(r.str("id")), Cell::from(r.str("snippet.label"))],
        )
        .map_err(OpError::new)
    }
}

impl Invoke for VideoAbuseReportReason {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(VideoAbuseReportReason {
        fields: Fields {
            hl: f.string("hl"),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<VideoAbuseReportReason>(args)
        .map(|v| Box::new(v) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "videoAbuseReportReason list",
    cli,
    mcp,
}];
