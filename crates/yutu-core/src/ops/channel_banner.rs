//! Port of pkg/channelBanner and cmd/channelBanner.

use super::common::{Fields, open_media};
use super::{Ctx, Entry, FlagValues, Invoke, OpError};
use crate::output::print_result;
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::client::BoxFuture;
use yutu_api::generated::CHANNEL_BANNER_RESOURCE;

const ERR_INSERT: &str = "failed to insert channelBanner";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ChannelBanner {
    #[serde(flatten)]
    pub fields: Fields,
    pub file: String,
    pub on_behalf_of_content_owner_channel: String,
}

impl ChannelBanner {
    pub async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let media = open_media(&self.file).map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let mut call = svc
            .call("youtube.channelBanners.insert")
            .body(&Value::new_struct(CHANNEL_BANNER_RESOURCE))
            .param("channelId", &self.fields.channel_id)
            .media(media);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        if !self.on_behalf_of_content_owner_channel.is_empty() {
            call = call.param(
                "onBehalfOfContentOwnerChannel",
                &self.on_behalf_of_content_owner_channel,
            );
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let url = res.str("url").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("ChannelBanner inserted: {url}\n"),
        )
        .map_err(OpError::new)
    }
}

impl Invoke for ChannelBanner {
    fn confirm(&self) -> String {
        format!(
            "Upload channel banner {} for channel {}",
            self.file, self.fields.channel_id
        )
    }

    fn run<'a>(&'a self, ctx: &'a mut Ctx<'_>) -> BoxFuture<'a, Result<(), OpError>> {
        Box::pin(self.insert(ctx))
    }
}

fn cli(f: &dyn FlagValues) -> Box<dyn Invoke> {
    Box::new(ChannelBanner {
        fields: Fields {
            channel_id: f.string("channelId"),
            on_behalf_of_content_owner: f.string("onBehalfOfContentOwner"),
            output: f.string("output"),
            ..Fields::default()
        },
        file: f.string("file"),
        on_behalf_of_content_owner_channel: f.string("onBehalfOfContentOwnerChannel"),
    })
}

fn mcp(args: serde_json::Value) -> Result<Box<dyn Invoke>, String> {
    serde_json::from_value::<ChannelBanner>(args)
        .map(|b| Box::new(b) as Box<dyn Invoke>)
        .map_err(|e| e.to_string())
}

pub static ENTRIES: [Entry; 1] = [Entry {
    path: "channelBanner insert",
    cli,
    mcp,
}];
