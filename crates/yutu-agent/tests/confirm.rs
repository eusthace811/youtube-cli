//! Divergence E9: a write the model asks for runs only after a human `y`.

mod common;

use common::{agent, block_on, console, mock_llm, recorder};
use serde_json::{Value, json};

#[test]
fn declining_a_delete_sends_nothing_to_youtube() {
    block_on(async {
        let (llm, model_log) = mock_llm(vec![
            json!({"calls": [{"name": "video-delete", "args": {"ids": ["v1"]}}]}),
            json!({"text": "I did not delete it."}),
        ])
        .await;
        let (youtube, api_log) = recorder().await;
        let agent = agent(&llm, &youtube).await;

        let transcript = console(&agent, &["delete video v1", "n"]).await;

        assert_eq!(
            transcript,
            "\n\nUser -> \nAgent -> \nTool call needs approval: video-delete {\"ids\":[\"v1\"]}\nDelete video(s): v1 [y/N] I did not delete it.\nUser -> \nEOF detected, exiting...\n"
        );
        assert!(api_log.lock().unwrap().is_empty());
        let model = model_log.lock().unwrap();
        let last: Value = serde_json::from_str(&model[1].1).unwrap();
        assert_eq!(
            last["contents"][2],
            json!({"parts": [{"functionResponse": {"name": "video-delete", "response":
                {"error": "Tool execution failed. Details: canceled by user"}}}], "role": "user"})
        );
    })
}

#[test]
fn end_of_input_at_the_prompt_declines() {
    block_on(async {
        let (llm, _) = mock_llm(vec![
            json!({"calls": [{"name": "playlist-delete", "args": {"ids": ["p1"]}}]}),
            json!({"text": "Kept it."}),
        ])
        .await;
        let (youtube, api_log) = recorder().await;
        let agent = agent(&llm, &youtube).await;

        let transcript = console(&agent, &["delete playlist p1"]).await;

        assert!(transcript.ends_with("[y/N] Kept it.\nUser -> \nEOF detected, exiting...\n"));
        assert!(api_log.lock().unwrap().is_empty());
    })
}
