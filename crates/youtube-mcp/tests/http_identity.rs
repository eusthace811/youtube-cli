//! In HTTP mode a tool call reaches YouTube with the caller's token only:
//! a poisoned `youtube.token.json` in the root is never read or sent.

mod common;

use common::{mcp_request, mock, serve, tokeninfo};
use serde_json::{Value, json};

fn videos(_line: &str, _body: &str) -> (u16, String) {
    (200, json!({"items": [{"id": "v1"}]}).to_string())
}

#[tokio::test]
async fn tool_calls_use_only_the_callers_token() {
    let root = std::env::temp_dir().join(format!("youtube-http-identity-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let poison = r#"{"access_token":"POISONED-FILE-TOKEN","token_type":"Bearer","refresh_token":"POISONED-REFRESH","expiry":"2099-01-01T00:00:00Z"}"#;
    std::fs::write(root.join("youtube.token.json"), poison).unwrap();
    std::fs::write(
        root.join("client_secret.json"),
        r#"{"installed":{"client_id":"POISONED"}}"#,
    )
    .unwrap();
    std::env::set_current_dir(&root).unwrap();

    let (ti, _) = mock(tokeninfo).await;
    let (api, seen) = mock(videos).await;
    let url = serve(format!("{ti}/tokeninfo"), None, format!("{api}/")).await;
    let client = common::client();
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "video-list", "arguments": {"ids": ["v1"]}}});
    let resp = mcp_request(&client, &url, call.to_string())
        .bearer_auth("good-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let reply: Value = serde_json::from_str(&resp.text().await.unwrap()).unwrap();
    assert_eq!(reply["result"]["isError"], Value::Null, "{reply}");
    assert_eq!(reply["result"]["content"][0]["text"], "[{\"id\":\"v1\"}]\n");

    let resp = mcp_request(&client, &url, call.to_string())
        .bearer_auth("unknown-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "only the verified caller reached the API");
    assert!(seen[0].request_line.starts_with("GET /youtube/v3/videos?"));
    let auth: Vec<&str> = seen[0]
        .head
        .lines()
        .filter(|l| l.to_ascii_lowercase().starts_with("authorization:"))
        .collect();
    assert_eq!(auth, ["authorization: Bearer good-token"]);
    assert!(!seen[0].head.contains("POISONED"));
    assert_eq!(
        std::fs::read_to_string(root.join("youtube.token.json")).unwrap(),
        poison,
        "the token file is untouched"
    );
    let _ = std::fs::remove_dir_all(&root);
}
