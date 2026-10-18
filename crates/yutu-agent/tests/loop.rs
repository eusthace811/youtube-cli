//! The agent loop against a scripted model: two reads run untouched, a write
//! waits for the human's `y`, and every result goes back to the model.

mod common;

use common::{agent, block_on, console, mock_llm, recorder};
use serde_json::{Value, json};

#[test]
fn three_tool_calls_reach_the_api_and_the_model() {
    block_on(async {
        let (llm, model_log) = mock_llm(vec![
            json!({"calls": [{"name": "video-list", "args": {"ids": ["v1"]}}]}),
            json!({"calls": [{"name": "channel-list", "args": {"mine": true}}]}),
            json!({"calls": [{"name": "playlist-insert", "args": {"title": "Best of", "channel_id": "c1", "privacy": "private"}}]}),
            json!({"text": "Created the playlist Best of."}),
        ])
        .await;
        let (youtube, api_log) = recorder().await;
        let agent = agent(&llm, &youtube).await;

        let transcript = console(&agent, &["make a playlist of my best videos", "y"]).await;

        assert_eq!(
            transcript,
            "\n\nUser -> \nAgent -> \u{18}\u{1b}\\\u{1b}[0m\nTool call needs approval: playlist-insert {\"channel_id\":\"c1\",\"privacy\":\"private\",\"title\":\"Best of\"}\nCreate playlist: Best of [y/N] Created the playlist Best of.\nUser -> \nEOF detected, exiting...\n"
        );
        let api: Vec<String> = api_log
            .lock()
            .unwrap()
            .iter()
            .map(|(line, _)| line.split('?').next().unwrap().to_string())
            .collect();
        assert_eq!(
            api,
            [
                "GET /youtube/v3/videos",
                "GET /youtube/v3/channels",
                "POST /youtube/v3/playlists"
            ]
        );
        let model = model_log.lock().unwrap();
        assert_eq!(model.len(), 4);
        let last: Value = serde_json::from_str(&model[3].1).unwrap();
        let responses: Vec<(&str, bool)> = last["contents"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|c| c["parts"].as_array().unwrap())
            .filter_map(|p| p.get("functionResponse"))
            .map(|r| {
                (
                    r["name"].as_str().unwrap(),
                    r["response"].get("output").is_some(),
                )
            })
            .collect();
        assert_eq!(
            responses,
            [
                ("video-list", true),
                ("channel-list", true),
                ("playlist-insert", true)
            ]
        );
    })
}
