//! Port of pkg/videoCategory and cmd/videoCategory. Go exposes list to MCP
//! only as the `video://category/{hl}` resource, not as a tool.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use youtube_api::Value;
use youtube_api::client::BoxFuture;

const ERR_GET: &str = "failed to get video categoryId";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VideoCategory {
    #[serde(flatten)]
    pub fields: Fields,
    pub region_code: String,
}

impl VideoCategory {
    pub async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.videoCategories.list")
            .params("part", &self.fields.parts);
        if !self.fields.ids.is_empty() {
            call = call.params("id", &self.fields.ids);
        }
        for (name, value) in [("hl", &self.fields.hl), ("regionCode", &self.region_code)] {
            if !value.is_empty() {
                call = call.param(name, value);
            }
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
            &["ID", "Title", "Assignable"],
            |c| {
                let assignable = matches!(c.at("snippet.assignable"), Some(Value::Bool(true)));
                vec![
                    Cell::from(c.str("id")),
                    Cell::from(c.str("snippet.title")),
                    Cell::from(assignable.to_string()),
                ]
            },
        )
        .map_err(OpError::new)
    }
}

impl Invoke for VideoCategory {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(VideoCategory {
        fields: Fields {
            ids: f.strings("ids"),
            hl: f.string("hl"),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
        region_code: f.string("regionCode"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<VideoCategory>(args)
        .map(|v| Box::new(v) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "videoCategory list",
    cli,
    mcp,
}];
