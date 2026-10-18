//! Divergence E9 in the web launchers: a write the model asks for in a dev
//! UI session pauses at ADK's tool confirmation request and runs only after
//! the human confirms it; without a human (bearer callers) it is declined.

mod common;

use common::{TOKEN, Web, block_on, http, web};
use serde_json::{Value, json};

const ARGS: &str = "web webui api";
const SESSION: &str = "/api/apps/Miffy/users/u/sessions/s";

fn script() -> Vec<Value> {
    vec![
        json!({"calls": [{"name": "playlist-delete", "args": {"ids": ["p1"]}}]}),
        json!({"text": "Done."}),
        json!({"text": "Next answer."}),
    ]
}

fn cookie() -> (&'static str, String) {
    ("Cookie", format!("yutu_token={TOKEN}"))
}

/// The SSE events of one `/run_sse` call.
async fn run_sse(w: &Web, auth: (&str, &str), message: Value) -> Vec<Value> {
    let body = json!({"appName": "Miffy", "userId": "u", "sessionId": "s", "newMessage": message});
    let r = http(
        w.port,
        "POST",
        "/api/run_sse",
        &[auth, ("Content-Type", "application/json")],
        body.to_string().as_bytes(),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text());
    r.text()
        .split("\n\n")
        .filter_map(|e| e.strip_prefix("data: "))
        .map(|d| serde_json::from_str(d).unwrap())
        .collect()
}

fn deletes(w: &Web) -> usize {
    w.youtube
        .lock()
        .unwrap()
        .iter()
        .filter(|(l, _)| l.starts_with("DELETE /youtube/v3/playlists"))
        .count()
}

async fn start(w: &Web, auth: (&str, &str)) {
    let r = http(w.port, "POST", SESSION, &[auth], b"").await;
    assert_eq!(r.status, 200);
}

fn text(message: &str) -> Value {
    json!({"role": "user", "parts": [{"text": message}]})
}

fn answer(request: &Value, confirmed: bool) -> Value {
    let call = &request["content"]["parts"][0]["functionCall"];
    json!({"role": "user", "parts": [{"functionResponse": {
        "id": call["id"], "name": "adk_request_confirmation",
        "response": {"confirmed": confirmed, "payload": call["args"]["originalFunctionCall"]["args"]}}}]})
}

/// The paused turn's last three events: the call, its "requires
/// confirmation" response, and the confirmation request.
fn assert_paused(events: &[Value]) -> Value {
    assert_eq!(events.len(), 3, "{events:?}");
    let call_id = events[0]["content"]["parts"][0]["functionCall"]["id"].clone();
    assert_eq!(
        events[1]["content"]["parts"][0]["functionResponse"],
        json!({"id": call_id, "name": "playlist-delete", "response": {
            "error": "error tool \"playlist-delete\" requires confirmation, please approve or reject"}})
    );
    assert_eq!(events[1]["actions"]["skipSummarization"], true);
    let request = events[2].clone();
    let fc = &request["content"]["parts"][0]["functionCall"];
    assert_eq!(fc["name"], "adk_request_confirmation");
    assert_eq!(request["longRunningToolIds"], json!([fc["id"]]));
    assert_eq!(
        fc["args"]["originalFunctionCall"],
        json!({"id": call_id, "args": {"ids": ["p1"]}, "name": "playlist-delete"})
    );
    assert_eq!(fc["args"]["toolConfirmation"]["confirmed"], false);
    assert!(
        fc["args"]["toolConfirmation"]["hint"]
            .as_str()
            .unwrap()
            .starts_with("Delete playlist(s): p1")
    );
    request
}

#[test]
fn a_ui_delete_waits_for_the_human_and_runs_once_confirmed() {
    block_on(async {
        let w = web(ARGS, script()).await;
        let (k, v) = cookie();
        start(&w, (k, &v)).await;
        let paused = run_sse(&w, (k, &v), text("delete playlist p1")).await;
        let request = assert_paused(&paused);
        assert_eq!(deletes(&w), 0, "nothing is deleted before approval");
        assert_eq!(w.model.lock().unwrap().len(), 1);

        let resumed = run_sse(&w, (k, &v), answer(&request, true)).await;
        assert_eq!(deletes(&w), 1);
        let response = &resumed[0]["content"]["parts"][0]["functionResponse"];
        assert_eq!(response["name"], "playlist-delete");
        assert!(response["response"].get("output").is_some(), "{response}");
        assert_eq!(resumed[1]["content"]["parts"][0]["text"], "Done.");

        let session: Value =
            serde_json::from_slice(&http(w.port, "GET", SESSION, &[(k, &v)], b"").await.body)
                .unwrap();
        let authors: Vec<&str> = session["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["author"].as_str().unwrap())
            .collect();
        assert_eq!(
            authors,
            ["user", "Miffy", "Miffy", "Miffy", "user", "Miffy", "Miffy"]
        );
    });
}

#[test]
fn a_rejected_ui_delete_sends_nothing() {
    block_on(async {
        let w = web(ARGS, script()).await;
        let (k, v) = cookie();
        start(&w, (k, &v)).await;
        let request = assert_paused(&run_sse(&w, (k, &v), text("delete playlist p1")).await);
        let resumed = run_sse(&w, (k, &v), answer(&request, false)).await;
        assert_eq!(deletes(&w), 0);
        assert_eq!(
            resumed[0]["content"]["parts"][0]["functionResponse"]["response"],
            json!({"error": "Tool execution failed. Details: canceled by user"})
        );
    });
}

#[test]
fn a_new_message_declines_the_pending_write_without_another_model_call() {
    block_on(async {
        let w = web(ARGS, script()).await;
        let (k, v) = cookie();
        start(&w, (k, &v)).await;
        assert_paused(&run_sse(&w, (k, &v), text("delete playlist p1")).await);
        let next = run_sse(&w, (k, &v), text("never mind")).await;
        assert_eq!(deletes(&w), 0);
        assert_eq!(next.len(), 1, "{next:?}");
        assert_eq!(next[0]["content"]["parts"][0]["text"], "Done.");
        let model = w.model.lock().unwrap();
        assert_eq!(model.len(), 2);
        let last: Value = serde_json::from_str(&model[1].1).unwrap();
        assert_eq!(
            last["contents"][2]["parts"][0]["functionResponse"]["response"],
            json!({"error": "Tool execution failed. Details: canceled by user"})
        );
        assert_eq!(last["contents"][3]["parts"][0]["text"], "never mind");
    });
}

#[test]
fn bearer_callers_have_no_human_so_writes_are_declined() {
    block_on(async {
        let w = web(ARGS, script()).await;
        let bearer = format!("Bearer {TOKEN}");
        let auth = ("Authorization", bearer.as_str());
        start(&w, auth).await;
        let events = run_sse(&w, auth, text("delete playlist p1")).await;
        assert_eq!(deletes(&w), 0);
        assert_eq!(events.len(), 3, "{events:?}");
        assert_eq!(
            events[1]["content"]["parts"][0]["functionResponse"]["response"],
            json!({"error": "Tool execution failed. Details: canceled by user"})
        );
        assert_eq!(events[2]["content"]["parts"][0]["text"], "Done.");
    });
}
