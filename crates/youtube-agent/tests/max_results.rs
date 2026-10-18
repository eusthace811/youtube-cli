//! Divergence E17: `max_results: 0` means no limit, as every tool schema
//! says, instead of fetching nothing.

mod common;

use common::{agent, block_on, console, cycling_pages, mock_llm, recorder};
use serde_json::json;

#[test]
fn zero_max_results_lists_everything() {
    block_on(async {
        let (llm, _) = mock_llm(vec![
            json!({"calls": [{"name": "playlist-list", "args": {"mine": true, "max_results": 0}}]}),
            json!({"text": "Here they are."}),
        ])
        .await;
        let (youtube, log) = recorder().await;
        let agent = agent(&llm, &youtube).await;

        console(&agent, &["list my playlists"]).await;

        let requests = log.lock().unwrap();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert!(
            requests[0].0.starts_with("GET /youtube/v3/playlists"),
            "{requests:?}"
        );
        assert!(requests[0].0.contains("maxResults=20"), "{requests:?}");
    })
}

#[test]
fn a_page_token_that_comes_back_ends_the_listing() {
    block_on(async {
        let (llm, _) = mock_llm(vec![
            json!({"calls": [{"name": "playlist-list", "args": {"mine": true, "max_results": 0}}]}),
            json!({"text": "Done."}),
        ])
        .await;
        let (youtube, log) = cycling_pages().await;
        let agent = agent(&llm, &youtube).await;

        console(&agent, &["list my playlists"]).await;

        let requests = log.lock().unwrap();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert!(requests[1].0.contains("pageToken=again"), "{requests:?}");
    })
}
