//! Port of pkg/membershipsLevel and cmd/membershipsLevel.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get memberships level";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MembershipsLevel {
    #[serde(flatten)]
    pub fields: Fields,
}

impl MembershipsLevel {
    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let res = svc
            .call("youtube.membershipsLevels.list")
            .params("part", &self.fields.parts)
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_GET, e))?;
        let items = res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default();
        print_list(
            &mut ctx.out,
            &self.fields.output,
            &items,
            &["ID", "Display Name"],
            |l| {
                vec![
                    Cell::from(l.str("id")),
                    Cell::from(l.str("snippet.levelDetails.displayName")),
                ]
            },
        )
        .map_err(OpError::new)
    }
}

impl Invoke for MembershipsLevel {
    fn confirm(&self) -> String {
        String::new()
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.list(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(MembershipsLevel {
        fields: Fields {
            parts: f.strings("parts"),
            output: f.string("output"),
            ..Fields::default()
        },
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<MembershipsLevel>(args)
        .map(|m| Box::new(m) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "membershipsLevel list",
    cli,
    mcp,
}];
