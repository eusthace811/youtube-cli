//! Port of pkg/thirdPartyLink and cmd/thirdPartyLink.

use super::common::Fields;
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::{Cell, print_list, print_result};
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::THIRD_PARTY_LINK;

const ERR_GET: &str = "failed to get third party link";
const ERR_INSERT: &str = "failed to insert third party link";
const ERR_UPDATE: &str = "failed to update third party link";
const ERR_DELETE: &str = "failed to delete third party link";

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
    Insert,
    Update,
    Delete,
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
        Verb::Insert | Verb::Update => {
            t.link_status = f.string("linkStatus");
            t.fields.parts = f.strings("parts");
            t.fields.output = f.string("output");
        }
        Verb::Delete => {}
    }
    Box::new(Bound(verb, t))
}

fn mcp(verb: Verb, args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    let t: ThirdPartyLink = serde_json::from_value(args).map_err(|e| e.to_string())?;
    Ok(Box::new(Bound(verb, t)))
}

pub static ENTRIES: [Entry; 4] = [
    Entry {
        path: "thirdPartyLink delete",
        cli: |f| cli(Verb::Delete, f),
        mcp: |a| mcp(Verb::Delete, a),
    },
    Entry {
        path: "thirdPartyLink insert",
        cli: |f| cli(Verb::Insert, f),
        mcp: |a| mcp(Verb::Insert, a),
    },
    Entry {
        path: "thirdPartyLink list",
        cli: |f| cli(Verb::List, f),
        mcp: |a| mcp(Verb::List, a),
    },
    Entry {
        path: "thirdPartyLink update",
        cli: |f| cli(Verb::Update, f),
        mcp: |a| mcp(Verb::Update, a),
    },
];

impl Invoke for Bound {
    fn confirm(&self) -> String {
        let token = &self.1.linking_token;
        match self.0 {
            Verb::Delete => format!("Delete third-party link(s): {token}"),
            Verb::Insert => format!("Create third-party link: {token}"),
            Verb::Update => format!("Update third-party link: {token}"),
            Verb::List => String::new(),
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        let t = &self.1;
        Box::pin(async move {
            match self.0 {
                Verb::List => t.list(ctx).await,
                Verb::Insert => t.insert(ctx).await,
                Verb::Update => t.update(ctx).await,
                Verb::Delete => t.delete(ctx).await,
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

    async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut link = Value::new_struct(THIRD_PARTY_LINK);
        link.as_struct_mut()
            .unwrap()
            .set_str("linkingToken", &self.linking_token);
        link.ensure("snippet").set_str("type", &self.r#type);
        link.ensure("status")
            .set_str("linkStatus", &self.link_status);
        let mut call = svc
            .call("youtube.thirdPartyLinks.insert")
            .params("part", &self.fields.parts)
            .body(&link);
        if !self.external_channel_id.is_empty() {
            call = call.param("externalChannelId", &self.external_channel_id);
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Third party link inserted: {}\n", res.str("linkingToken")),
        )
        .map_err(OpError::new)
    }

    async fn update(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        ctx.service().await?;
        // Go looks the link up without its type, the new value to set.
        let lookup = ThirdPartyLink {
            r#type: String::new(),
            ..self.clone()
        };
        let links = lookup
            .get(ctx)
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        let Some(mut link) = links.into_iter().next() else {
            return Err(OpError::new(ERR_GET));
        };
        if !self.link_status.is_empty() {
            link.ensure("status")
                .set_str("linkStatus", &self.link_status);
        }
        if !self.r#type.is_empty() {
            link.ensure("snippet").set_str("type", &self.r#type);
        }

        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.thirdPartyLinks.update")
            .params("part", &self.fields.parts)
            .body(&link);
        if !self.external_channel_id.is_empty() {
            call = call.param("externalChannelId", &self.external_channel_id);
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_UPDATE, e))?;
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Third party link updated: {}\n", res.str("linkingToken")),
        )
        .map_err(OpError::new)
    }

    async fn delete(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut call = svc
            .call("youtube.thirdPartyLinks.delete")
            .param("linkingToken", &self.linking_token)
            .param("type", &self.r#type);
        if !self.external_channel_id.is_empty() {
            call = call.param("externalChannelId", &self.external_channel_id);
        }
        if !self.fields.parts.is_empty() {
            call = call.params("part", &self.fields.parts);
        }
        call.send_empty()
            .await
            .map_err(|e| OpError::wrap(ERR_DELETE, e))?;
        ctx.out
            .write_str(&format!(
                "Third party link deleted: {}\n",
                self.linking_token
            ))
            .map_err(OpError::new)
    }
}
