//! Port of pkg/playlistItem and cmd/playlistItem.

use super::common::Fields;
use super::{Ctx, OpError};
use crate::output::print_result;
use serde::Deserialize;
use yutu_api::Value;
use yutu_api::generated::PLAYLIST_ITEM;

const ERR_INSERT: &str = "failed to insert playlist item";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlaylistItem {
    #[serde(flatten)]
    pub fields: Fields,
    pub title: String,
    pub description: Option<String>,
    pub kind: String,
    pub k_video_id: String,
    pub k_channel_id: String,
    pub k_playlist_id: String,
    pub video_id: String,
    pub playlist_id: String,
    pub privacy: String,
}

impl PlaylistItem {
    pub async fn insert(&self, ctx: &mut Ctx<'_>) -> Result<(), OpError> {
        let svc = ctx.service().await?;
        let mut item = Value::new_struct(PLAYLIST_ITEM);
        {
            let s = item.ensure("snippet");
            s.set_str("title", &self.title)
                .set_str("playlistId", &self.playlist_id)
                .set_str("channelId", &self.fields.channel_id);
            if let Some(d) = &self.description {
                s.set_str("description", d);
            }
        }
        let resource = match self.kind.as_str() {
            "video" => Some(("youtube#video", "videoId", &self.k_video_id)),
            "channel" => Some(("youtube#channel", "channelId", &self.k_channel_id)),
            "playlist" => Some(("youtube#playlist", "playlistId", &self.k_playlist_id)),
            _ => None,
        };
        if let Some((kind, key, id)) = resource {
            item.ensure("snippet.resourceId")
                .set_str("kind", kind)
                .set_str(key, id);
        }
        item.ensure("status")
            .set_str("privacyStatus", &self.privacy);
        let mut call = svc
            .call("youtube.playlistItems.insert")
            .params("part", &["snippet".into(), "status".into()])
            .body(&item);
        if !self.fields.on_behalf_of_content_owner.is_empty() {
            call = call.param(
                "onBehalfOfContentOwner",
                &self.fields.on_behalf_of_content_owner,
            );
        }
        let res = call
            .send()
            .await
            .map_err(|e| OpError::wrap(ERR_INSERT, e))?;
        let id = res.str("id").to_string();
        print_result(
            &mut ctx.out,
            &self.fields.output,
            &res,
            &format!("Playlist Item inserted: {id}\n"),
        )
        .map_err(OpError::new)
    }
}

pub static ENTRIES: [super::Entry; 0] = [];
