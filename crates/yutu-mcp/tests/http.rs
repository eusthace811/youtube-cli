//! `yutu mcp --mode http` against a mock tokeninfo: go-sdk's bearer-token
//! refusals, the body cap, and the E4 and E8 hardening.

mod common;

use common::{BASE, SCOPES, mcp_request, mock, serve, tokeninfo};

const PING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}"#;

fn challenge() -> String {
    format!(
        r#"Bearer resource_metadata="{BASE}/.well-known/oauth-protected-resource", scope="{SCOPES}""#
    )
}

async fn send(req: reqwest::RequestBuilder) -> (u16, Option<String>, String) {
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let auth = resp
        .headers()
        .get("www-authenticate")
        .map(|v| v.to_str().unwrap().to_string());
    (status, auth, resp.text().await.unwrap())
}

#[tokio::test]
async fn refuses_requests_without_a_bearer_token() {
    let (ti, _) = mock(tokeninfo).await;
    let url = serve(format!("{ti}/tokeninfo"), None, String::new()).await;
    let client = common::client();
    let got = send(mcp_request(&client, &url, PING)).await;
    assert_eq!(got, (401, Some(challenge()), "no bearer token\n".into()));
    let got = send(mcp_request(&client, &url, PING).bearer_auth("unknown")).await;
    assert_eq!(
        got,
        (
            401,
            Some(challenge()),
            "invalid token: tokeninfo returned 400\n".into()
        )
    );
}

/// go-sdk answers a valid token that lacks a scope with 403, not 401.
#[tokio::test]
async fn refuses_tokens_missing_a_scope() {
    let (ti, _) = mock(tokeninfo).await;
    let url = serve(format!("{ti}/tokeninfo"), None, String::new()).await;
    let client = common::client();
    let got = send(mcp_request(&client, &url, PING).bearer_auth("few-scopes")).await;
    assert_eq!(got, (403, Some(challenge()), "insufficient scope\n".into()));
}

#[tokio::test]
async fn refuses_foreign_origins() {
    let (ti, _) = mock(tokeninfo).await;
    let url = serve(format!("{ti}/tokeninfo"), None, String::new()).await;
    let client = common::client();
    let req = mcp_request(&client, &url, PING)
        .bearer_auth("good-token")
        .header("Origin", "https://evil.example");
    assert_eq!(
        send(req).await,
        (
            403,
            None,
            "Forbidden: invalid Origin header \"https://evil.example\"\n".into()
        )
    );
    let req = mcp_request(&client, &url, PING)
        .bearer_auth("good-token")
        .header("Origin", "http://localhost:3000");
    assert_eq!(
        send(req).await,
        (200, None, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.into())
    );
}

fn padded(size: usize) -> String {
    let head = r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"x":""#;
    let tail = r#""}}"#;
    format!("{head}{}{tail}", "a".repeat(size - head.len() - tail.len()))
}

#[tokio::test]
async fn caps_bodies_at_two_mebibytes() {
    let (ti, _) = mock(tokeninfo).await;
    let url = serve(format!("{ti}/tokeninfo"), None, String::new()).await;
    let client = common::client();
    let req = mcp_request(&client, &url, padded((2 << 20) + 1)).bearer_auth("good-token");
    assert_eq!(
        send(req).await,
        (413, None, "request body exceeds 2097152 bytes\n".into())
    );
    let req = mcp_request(&client, &url, padded(2 << 20)).bearer_auth("good-token");
    assert_eq!(send(req).await.0, 200);
}

/// E4: the token travels in a form body, never the URL, and is verified once
/// until it expires.
#[tokio::test]
async fn posts_the_token_and_caches_the_verdict() {
    let (ti, seen) = mock(tokeninfo).await;
    let url = serve(format!("{ti}/tokeninfo"), None, String::new()).await;
    let client = common::client();
    for _ in 0..3 {
        let req = mcp_request(&client, &url, PING).bearer_auth("good-token");
        assert_eq!(send(req).await.0, 200);
    }
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].request_line, "POST /tokeninfo HTTP/1.1");
    assert_eq!(seen[0].body, "access_token=good-token");
    assert!(
        seen[0]
            .head
            .to_ascii_lowercase()
            .contains("content-type: application/x-www-form-urlencoded")
    );
}

/// E4: with an allowlist, a token issued to another client is refused.
#[tokio::test]
async fn enforces_the_client_allowlist() {
    let (ti, _) = mock(tokeninfo).await;
    let client = common::client();
    let url = serve(
        format!("{ti}/tokeninfo"),
        Some(vec!["client-b".into()]),
        String::new(),
    )
    .await;
    let got = send(mcp_request(&client, &url, PING).bearer_auth("good-token")).await;
    assert_eq!(
        got,
        (
            401,
            Some(challenge()),
            "invalid token: audience not allowed\n".into()
        )
    );
    let url = serve(
        format!("{ti}/tokeninfo"),
        Some(vec!["client-b".into(), "client-a".into()]),
        String::new(),
    )
    .await;
    let got = send(mcp_request(&client, &url, PING).bearer_auth("good-token")).await;
    assert_eq!(got.0, 200);
}

#[tokio::test]
async fn serves_protected_resource_metadata_and_404s() {
    let (ti, _) = mock(tokeninfo).await;
    let url = serve(format!("{ti}/tokeninfo"), None, String::new()).await;
    let root = url.trim_end_matches("/mcp");
    let client = common::client();
    let resp = client
        .get(format!("{root}/.well-known/oauth-protected-resource"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.headers()["access-control-allow-origin"], "*");
    assert_eq!(
        resp.text().await.unwrap(),
        format!(
            "{{\"resource\":\"{BASE}/mcp\",\"authorization_servers\":[\"https://accounts.google.com\"],\"scopes_supported\":[{}]}}\n",
            SCOPES
                .split(' ')
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(",")
        )
    );
    let resp = client.get(format!("{root}/nope")).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 404);
    assert_eq!(resp.text().await.unwrap(), "404 page not found\n");
}
