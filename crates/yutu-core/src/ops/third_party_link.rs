//! Port of pkg/thirdPartyLink and cmd/thirdPartyLink.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;

const ERR_GET: &str = "failed to get third party link";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThirdPartyLink {
    #[serde(flatten)]
    pub fields: Fields,
    pub linking_token: String,
    pub r#type: String,
    pub link_status: String,
    pub external_channel_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Verb {
    List,
}

struct Bound(Verb, ThirdPartyLink);

fn cli(verb: Verb, f: &dyn FlagValues) -> Box<dyn Invoke> {
    let mut t = ThirdPartyLink {
        linking_token: f.string("linkingToken"),
        r#type: f.string("type"),
        external_channel_id: f.string("externalChannelId"),
        ..ThirdPartyLink::default()
    };
    match verb {
        Verb::List => {
            t.fields.parts = f.strings("parts");
            t.fields.output = f.string("output");
        }
    }
    Box::new(Bound(verb, t))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let t: ThirdPartyLink = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, t)))
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "thirdPartyLink list",
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
        let t = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => t.list(ctx).await,
            }
        })
    }
}

impl ThirdPartyLink {
    async fn get(&self, ctx: &mut Ctx<'_>) -> Result<Vec<Value>, OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.thirdPartyLinks.list")
            .params("part", &self.fields.parts);
        for (name, value) in [
            ("linkingToken", &self.linking_token),
            ("type", &self.r#type),
            ("externalChannelId", &self.external_channel_id),
        ] {
            if !value.is_empty() {
                call = call.param(name, value);
            }
        }
        let res = call.send().await.map_err(|e| OpError::wrap(ERR_GET, e))?;
        Ok(res
            .at("items")
            .map(|v| v.as_list().to_vec())
            .unwrap_or_default())
    }

    async fn list(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let links = self.get(ctx).await?;
        print_list(
            &mut ctx.out,
            &self.fields.output,
            &links,
            &["Linking Token", "Type", "Link Status"],
            |link| {
                vec![
                    Cell::from(link.str("linkingToken")),
                    Cell::from(link.str("snippet.type")),
                    Cell::from(link.str("status.linkStatus")),
                ]
            },
        )
        .map_err(OpError::new)
    }
}
