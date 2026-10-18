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
            "\n\nUser -> \nAgent -> \u{18}\u{1b}\\\u{1b}[0m\nTool call needs approval: video-delete {\"ids\":[\"v1\"]}\nDelete video(s): v1 [y/N] I did not delete it.\nUser -> \nEOF detected, exiting...\n"
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

async fn queued_session(stdin: &[&str], interactive: bool) -> (String, usize) {
    let (llm, _) = mock_llm(vec![
        json!({"calls": [{"name": "video-delete", "args": {"ids": ["v1"]}}]}),
        json!({"text": "Kept it."}),
        json!({"text": "ok"}),
    ])
    .await;
    let (youtube, api_log) = recorder().await;
    let agent = agent(&llm, &youtube).await;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for line in stdin {
        tx.send(youtube_agent::console::Line::now(format!("{line}\n")))
            .unwrap();
    }
    drop(tx);
    let mut out: Vec<u8> = Vec::new();
    youtube_agent::console::run(
        &agent,
        rx,
        &mut out,
        youtube_agent::launcher::Streaming::None,
        interactive,
        interactive,
    )
    .await;
    let writes = api_log.lock().unwrap().len();
    (String::from_utf8(out).unwrap(), writes)
}

#[test]
fn a_y_queued_before_the_prompt_does_not_approve() {
    block_on(async {
        let (transcript, writes) = queued_session(&["delete video v1", "y"], true).await;

        assert_eq!(writes, 0);
        assert!(transcript.contains("[y/N] Kept it.\nUser -> \nAgent -> ok"));
    })
}

#[test]
fn without_a_terminal_every_write_is_declined() {
    block_on(async {
        let (transcript, writes) = queued_session(&["delete video v1"], false).await;

        assert_eq!(writes, 0);
        assert!(transcript.contains("[y/N] \nnot a terminal, declined\nKept it."));
    })
}

#[test]
fn model_text_cannot_hide_the_question() {
    block_on(async {
        let (llm, _) = mock_llm(vec![
            json!({"text": "Listing.\u{1b}]0;title", "calls": [{"name": "video-delete", "args": {"ids": ["v1\u{202e}"]}}]}),
            json!({"text": "ok"}),
        ])
        .await;
        let (youtube, _) = recorder().await;
        let agent = agent(&llm, &youtube).await;

        let transcript = console(&agent, &["go", "n"]).await;

        let question = transcript.find("\nTool call needs approval").unwrap();
        assert!(
            transcript[..question].ends_with("\u{18}\u{1b}\\\u{1b}[0m"),
            "open sequences are cancelled and attributes reset before the question: {transcript:?}"
        );
        assert!(transcript.contains("v1\\u{202e}"), "{transcript:?}");
    })
}

#[test]
fn control_sequences_in_model_text_are_shown_not_obeyed() {
    block_on(async {
        let payload =
            "Sure.\u{1b}]10;#000000\u{7}\u{1b}]11;#000000\u{7}\u{1b}(0\u{1b}[?69h\u{1b}[1;1s";
        let (llm, _) = mock_llm(vec![
            json!({"text": payload}),
            json!({"calls": [{"name": "video-delete", "args": {"ids": ["v1"]}}]}),
            json!({"text": "ok"}),
        ])
        .await;
        let (youtube, _) = recorder().await;
        let agent = agent(&llm, &youtube).await;

        let transcript = console(&agent, &["hi", "go", "n"]).await;

        assert!(
            transcript.contains("Sure.\\u{1b}]10;#000000\\u{7}\\u{1b}]11;#000000"),
            "{transcript:?}"
        );
        assert_eq!(
            transcript.matches('\u{1b}').count(),
            2,
            "only the console's own reset reaches the terminal raw: {transcript:?}"
        );
    })
}

#[test]
fn piped_input_with_terminal_output_still_escapes_model_text() {
    block_on(async {
        let (llm, _) = mock_llm(vec![
            json!({"text": "hi\u{1b}]52;c;Zm9v\u{7}\u{1b}[8m hidden"}),
        ])
        .await;
        let (youtube, _) = recorder().await;
        let agent = agent(&llm, &youtube).await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(youtube_agent::console::Line::now("go\n")).unwrap();
        drop(tx);
        let mut out: Vec<u8> = Vec::new();
        youtube_agent::console::run(
            &agent,
            rx,
            &mut out,
            youtube_agent::launcher::Streaming::None,
            false,
            true,
        )
        .await;
        let transcript = String::from_utf8(out).unwrap();

        assert!(
            transcript.contains("hi\\u{1b}]52;c;Zm9v\\u{7}\\u{1b}[8m hidden"),
            "{transcript:?}"
        );
        assert!(!transcript.contains('\u{1b}'), "{transcript:?}");
    })
}
