//! Port of pkg/i18nRegion and cmd/i18nRegion. The CLI command has no MCP
//! tool; its MCP binding backs the i18n region resource.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use youtube_api::client::BoxFuture;

const ERR_GET: &str = "failed to get i18n region";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct I18nRegion {
    #[serde(flatten)]
    pub fields: Fields,
}

impl I18nRegion {
    pub async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.i18nRegions.list")
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
            &["ID", "Gl", "Name"],
            |r| {
                vec![
                    Cell::from(r.str("id")),
                    Cell::from(r.str("snippet.gl")),
                    Cell::from(r.str("snippet.name")),
                ]
            },
        )
        .map_err(OpError::new)
    }
}

impl Invoke for I18nRegion {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(I18nRegion {
        fields: Fields {
            hl: f.string("hl"),
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<I18nRegion>(args)
        .map(|r| Box::new(r) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "i18nRegion list",
    cli,
    mcp,
}];
