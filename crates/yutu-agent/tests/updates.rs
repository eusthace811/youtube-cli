//! Divergence E18: the four updates whose lookup Go never let fetch
//! anything now send requests the YouTube API accepts, checked against
//! the parameter rules in spec/youtube-v3.json.

mod common;

use common::{agent, block_on, console, mock_llm, recorder};
use serde_json::{Value, json};

/// The requests one approved tool call sends, with `%2C` shown as `,`.
async fn approved(tool: &str, args: Value) -> Vec<String> {
    let (llm, _) = mock_llm(vec![
        json!({"calls": [{"name": tool, "args": args}]}),
        json!({"text": "Done."}),
    ])
    .await;
    let (youtube, log) = recorder().await;
    let agent = agent(&llm, &youtube).await;
    console(&agent, &["change it", "y"]).await;
    let requests = log.lock().unwrap();
    requests
        .iter()
        .map(|(line, _)| line.replace("%2C", ","))
        .collect()
}

/// The values of a parameter sent once or repeated, joined with commas.
fn param(line: &str, name: &str) -> String {
    query(line)
        .iter()
        .filter_map(|p| p.strip_prefix(&format!("{name}=")))
        .collect::<Vec<_>>()
        .join(",")
}

fn query(line: &str) -> Vec<&str> {
    line.split_once('?')
        .map_or("", |(_, q)| q.split(' ').next().unwrap_or(""))
        .split('&')
        .collect()
}

#[test]
fn playlist_update_looks_up_with_the_required_parts() {
    block_on(async {
        let r = approved("playlist-update", json!({"ids": ["PL1"], "title": "New"})).await;
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].starts_with("GET /youtube/v3/playlists?"), "{r:?}");
        assert_eq!(param(&r[0], "part"), "id,snippet,status");
        assert_eq!(param(&r[0], "id"), "PL1");
        assert!(r[1].starts_with("PUT /youtube/v3/playlists?"), "{r:?}");
        assert_eq!(param(&r[1], "part"), "snippet,status");
    })
}

#[test]
fn playlist_item_update_looks_up_then_writes() {
    block_on(async {
        let r = approved(
            "playlistItem-update",
            json!({"ids": ["PI1"], "title": "New"}),
        )
        .await;
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].starts_with("GET /youtube/v3/playlistItems?"), "{r:?}");
        assert_eq!(param(&r[0], "part"), "id,snippet,status");
        assert!(r[1].starts_with("PUT /youtube/v3/playlistItems?"), "{r:?}");
    })
}

#[test]
fn channel_update_writes_branding_settings() {
    block_on(async {
        let r = approved(
            "channel-update",
            json!({"ids": ["UC1"], "description": "About"}),
        )
        .await;
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].starts_with("GET /youtube/v3/channels?"), "{r:?}");
        assert_eq!(param(&r[0], "part"), "brandingSettings");
        assert!(r[1].starts_with("PUT /youtube/v3/channels?"), "{r:?}");
        assert_eq!(param(&r[1], "part"), "brandingSettings");
    })
}

#[test]
fn channel_custom_url_is_refused_before_any_request() {
    block_on(async {
        let r = approved("channel-update", json!({"ids": ["UC1"], "custom_url": "x"})).await;
        assert!(r.is_empty(), "{r:?}");
    })
}

#[test]
fn comment_update_looks_up_by_id_without_a_count() {
    block_on(async {
        let r = approved(
            "comment-update",
            json!({"ids": ["C1"], "text_original": "Hi"}),
        )
        .await;
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].starts_with("GET /youtube/v3/comments?"), "{r:?}");
        assert_eq!(param(&r[0], "id"), "C1");
        assert_eq!(param(&r[0], "maxResults"), "", "{r:?}");
        assert!(r[1].starts_with("PUT /youtube/v3/comments?"), "{r:?}");
    })
}
