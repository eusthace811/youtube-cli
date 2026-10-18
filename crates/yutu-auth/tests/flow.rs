mod common;

use common::{MockTokenEndpoint, SharedOut, TempDir, client_secret, free_port, get, query_value};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use yutu_api::client::TokenSource;
use yutu_auth::flow::s256_challenge;
use yutu_auth::{Options, Prompt, RootDir, Token, authorize};

/// A prompt whose "browser" hands the consent URL to the test, which then
/// plays the user by calling the loopback callback itself.
fn browser_prompt() -> (Prompt, mpsc::UnboundedReceiver<String>, SharedOut) {
    let (tx, rx) = mpsc::unbounded_channel();
    let out = SharedOut::default();
    let prompt = Prompt {
        open_url: Box::new(move |url| {
            tx.send(url.to_string()).unwrap();
            Ok(())
        }),
        out: Box::new(out.clone()),
        input: None,
    };
    (prompt, rx, out)
}

fn options(port: u16, timeout: Duration) -> Options {
    Options {
        redirect_url: format!("http://127.0.0.1:{port}"),
        timeout,
    }
}

fn form_body(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_state_gets_400_and_no_exchange() {
    let mock = MockTokenEndpoint::start(vec![]).await;
    let dir = TempDir::new();
    let root = Arc::new(RootDir::open(dir.str()).unwrap());
    let token_file = dir.join("youtube.token.json");
    let port = free_port();
    let (prompt, mut urls, _) = browser_prompt();
    let secret = client_secret(&mock.url);
    let flow = authorize(
        root,
        &secret,
        token_file.to_str().unwrap(),
        options(port, Duration::from_millis(700)),
        prompt,
    );
    let user = async {
        let url = urls.recv().await.unwrap();
        assert_eq!(
            query_value(&url, "redirect_uri"),
            format!("http://127.0.0.1:{port}")
        );
        get(port, "/?state=forged&code=4/stolen").await
    };
    let (result, response) = tokio::join!(flow, user);
    assert_eq!(
        response,
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nX-Content-Type-Options: nosniff\r\nContent-Length: 42\r\nConnection: close\r\n\r\nstate doesn't match, possible CSRF attack\n"
    );
    assert_eq!(result.err().unwrap(), "authorization timed out after 700ms");
    assert_eq!(mock.recorded().len(), 0);
    assert!(!token_file.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn code_is_exchanged_with_the_verifier_and_refreshes_persist() {
    let mock = MockTokenEndpoint::start(vec![
        r#"{"access_token":"at-1","token_type":"Bearer","refresh_token":"1//rt","expires_in":11}"#,
        r#"{"access_token":"at-2","token_type":"Bearer","expires_in":3599}"#,
    ])
    .await;
    let dir = TempDir::new();
    let root = Arc::new(RootDir::open(dir.str()).unwrap());
    let token_file = dir.join("nested/youtube.token.json");
    let port = free_port();
    let (prompt, mut urls, out) = browser_prompt();
    let secret = client_secret(&mock.url);
    let flow = authorize(
        root,
        &secret,
        token_file.to_str().unwrap(),
        options(port, Duration::from_secs(5)),
        prompt,
    );
    let user = async {
        let url = urls.recv().await.unwrap();
        let state = query_value(&url, "state");
        assert_eq!(state.len(), 172);
        let response = get(port, &format!("/?state={state}&code=4/granted&scope=x")).await;
        (url, response)
    };
    let (source, (url, response)) = tokio::join!(flow, user);
    let source = source.unwrap();
    assert_eq!(
        response,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 84\r\nConnection: close\r\n\r\nAuthorization code received: 4/granted\nYou can now safely close the browser window.\n"
    );
    assert_eq!(
        out.text(),
        format!(
            "Your browser has been opened to an authorization URL. yutu will resume once authorization has been provided.\n{url}\n"
        )
    );

    let recorded = mock.recorded();
    assert_eq!(recorded.len(), 1);
    let challenge = query_value(&url, "code_challenge");
    let verifier = recorded[0]
        .body
        .split('&')
        .find_map(|p| p.strip_prefix("code_verifier="))
        .unwrap()
        .to_string();
    assert_eq!(s256_challenge(&verifier), challenge);
    assert_eq!(
        recorded[0].body,
        form_body(&[
            ("code", "4%2Fgranted"),
            ("code_verifier", &verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &format!("http%3A%2F%2F127.0.0.1%3A{port}")),
        ])
    );
    // oauth2 tries HTTP basic auth first: base64("cid.apps.googleusercontent.com:sec").
    assert!(
        recorded[0]
            .head
            .contains("authorization: Basic Y2lkLmFwcHMuZ29vZ2xldXNlcmNvbnRlbnQuY29tOnNlYw==")
    );

    let saved = Token::from_go_json(&std::fs::read(&token_file).unwrap()).unwrap();
    assert_eq!(
        (
            saved.access_token.as_str(),
            saved.refresh_token.as_str(),
            saved.expires_in
        ),
        ("at-1", "1//rt", 11)
    );
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&token_file), 0o600);
    assert_eq!(mode(&dir.join("nested")), 0o755);

    // expires_in 11 leaves the token valid for about one second.
    assert_eq!(source.token().await.unwrap(), "at-1");
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(source.token().await.unwrap(), "at-2");

    let recorded = mock.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[1].body,
        "grant_type=refresh_token&refresh_token=1%2F%2Frt"
    );
    let bytes = std::fs::read(&token_file).unwrap();
    let refreshed = Token::from_go_json(&bytes).unwrap();
    assert_eq!(
        (
            refreshed.access_token.as_str(),
            refreshed.refresh_token.as_str(),
            refreshed.expires_in
        ),
        ("at-2", "1//rt", 3599)
    );
    assert_eq!(refreshed.to_go_json(), bytes);
    assert_eq!(mode(&token_file), 0o600);
    let leftovers: Vec<_> = std::fs::read_dir(dir.join("nested"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, vec!["youtube.token.json"]);
}

#[tokio::test(flavor = "current_thread")]
async fn pasted_code_is_used_when_the_browser_fails() {
    let mock = MockTokenEndpoint::start(vec![
        r#"{"access_token":"at-1","refresh_token":"1//rt","expires_in":3599}"#,
    ])
    .await;
    let dir = TempDir::new();
    let root = Arc::new(RootDir::open(dir.str()).unwrap());
    let port = free_port();
    let out = SharedOut::default();
    let prompt = Prompt {
        open_url: Box::new(|_| Err(std::io::Error::other("no browser"))),
        out: Box::new(out.clone()),
        input: Some(Box::new(&b"\n  4%2Fpasted extra\n"[..])),
    };
    let secret = client_secret(&mock.url);
    let source = authorize(
        root,
        &secret,
        "",
        options(port, Duration::from_secs(5)),
        prompt,
    )
    .await
    .unwrap();
    assert_eq!(source.token().await.unwrap(), "at-1");
    assert!(mock.recorded()[0].body.starts_with("code=4%2Fpasted&"));
    let text = out.text();
    assert!(text.starts_with(
        "It seems that your browser is not open. Go to the following link in your browser:\nhttps://accounts.google.com/o/oauth2/auth?access_type=offline&"
    ));
    assert!(text.ends_with(&format!(
        "\n\nAfter completing the authorization flow, enter the authorization code on command line.\n\nIf you end up in an error page after completing the authorization flow,\nand the url in the address bar is in the form of\n'http://127.0.0.1:{port}/?state=DONOT-COPY&code=COPY-THIS&scope=DONOT-COPY'\nONLY 'COPY-THIS' part is the code you need to enter on command line.\n"
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn without_stdin_the_flow_waits_for_the_callback() {
    let mock = MockTokenEndpoint::start(vec![
        r#"{"access_token":"at-1","refresh_token":"1//rt","expires_in":3599}"#,
    ])
    .await;
    let dir = TempDir::new();
    let root = Arc::new(RootDir::open(dir.str()).unwrap());
    let port = free_port();
    let out = SharedOut::default();
    let (tx, mut urls) = mpsc::unbounded_channel();
    let prompt = Prompt {
        open_url: Box::new(move |url| {
            tx.send(url.to_string()).unwrap();
            Err(std::io::Error::other("no browser"))
        }),
        out: Box::new(out.clone()),
        input: None,
    };
    let secret = client_secret(&mock.url);
    let flow = authorize(
        root,
        &secret,
        "",
        options(port, Duration::from_secs(5)),
        prompt,
    );
    let user = async {
        let url = urls.recv().await.unwrap();
        let state = query_value(&url, "state");
        tokio::time::sleep(Duration::from_millis(50)).await;
        get(port, &format!("/?state={state}&code=cb")).await
    };
    let (source, response) = tokio::join!(flow, user);
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(source.unwrap().token().await.unwrap(), "at-1");
    assert!(mock.recorded()[0].body.starts_with("code=cb&"));
    assert!(
        out.text()
            .starts_with("It seems that your browser is not open.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn expired_token_without_a_file_fails_like_go() {
    let mock = MockTokenEndpoint::start(vec![
        r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
        r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
    ])
    .await;
    let dir = TempDir::new();
    let root = Arc::new(RootDir::open(dir.str()).unwrap());
    let (prompt, _urls, _) = browser_prompt();
    let expired = r#"{"access_token":"old","refresh_token":"1//rt","expiry":"2020-01-01T00:00:00Z","expires_in":0}"#;
    let secret = client_secret(&mock.url);
    let err = authorize(
        root,
        &secret,
        expired,
        options(free_port(), Duration::from_secs(1)),
        prompt,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(
        err,
        "failed to refresh token, please re-authenticate in cli: oauth2: \"invalid_grant\" \"Bad Request\""
    );
    // Basic auth failed, so oauth2 retried with the client in the form.
    let recorded = mock.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[1].body,
        "client_id=cid.apps.googleusercontent.com&client_secret=sec&grant_type=refresh_token&refresh_token=1%2F%2Frt"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn other_paths_get_404() {
    let mock = MockTokenEndpoint::start(vec![]).await;
    let dir = TempDir::new();
    let root = Arc::new(RootDir::open(dir.str()).unwrap());
    let port = free_port();
    let (prompt, mut urls, _) = browser_prompt();
    let secret = client_secret(&mock.url);
    let flow = authorize(
        root,
        &secret,
        "",
        options(port, Duration::from_millis(300)),
        prompt,
    );
    let user = async {
        urls.recv().await.unwrap();
        get(port, "/favicon.ico").await
    };
    let (result, response) = tokio::join!(flow, user);
    assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(response.ends_with("\r\n\r\n404 page not found\n"));
    assert_eq!(result.err().unwrap(), "authorization timed out after 300ms");
}
