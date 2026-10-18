//! Divergence E11: every route of every web sublauncher answers 401 unless
//! the request carries the per-launch token, as a bearer header or as the
//! cookie the dev UI's sign-in link sets.

mod common;

use common::{TOKEN, UI_TOKEN, block_on, http, web};

const ARGS: &str = "web webui a2a pubsub eventarc api -include_debug_api";

/// Every route the launchers serve: the 39 the dev UI calls (ADK's
/// uicontract_test.go), the rest of the REST API, the debug API, A2A, the
/// triggers, the UI's own files, and a path nothing serves.
const ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/run_sse"),
    ("GET", "/api/list-apps"),
    ("GET", "/api/version"),
    ("GET", "/api/apps/Miffy/users/u/sessions"),
    ("POST", "/api/apps/Miffy/users/u/sessions"),
    ("GET", "/api/apps/Miffy/users/u/sessions/s"),
    ("PATCH", "/api/apps/Miffy/users/u/sessions/s"),
    ("DELETE", "/api/apps/Miffy/users/u/sessions/s"),
    ("GET", "/api/apps/Miffy/users/u/sessions/s/artifacts/a"),
    (
        "GET",
        "/api/apps/Miffy/users/u/sessions/s/artifacts/a/versions/0",
    ),
    ("GET", "/api/run_live?appName=Miffy&userId=u&sessionId=s"),
    ("GET", "/api/dev/apps/Miffy/build_graph"),
    ("GET", "/api/dev/apps/Miffy/build_graph_image"),
    ("GET", "/api/dev/apps/Miffy/builder"),
    ("POST", "/api/dev/apps/Miffy/builder/save"),
    ("POST", "/api/dev/apps/Miffy/builder/cancel"),
    ("GET", "/api/dev/apps/Miffy/eval_sets"),
    ("POST", "/api/dev/apps/Miffy/eval-sets"),
    ("GET", "/api/dev/apps/Miffy/eval_sets/e"),
    ("DELETE", "/api/dev/apps/Miffy/eval_sets/e"),
    ("GET", "/api/dev/apps/Miffy/eval_sets/e/evals"),
    ("GET", "/api/dev/apps/Miffy/eval_sets/e/evals/c"),
    ("PUT", "/api/dev/apps/Miffy/eval_sets/e/evals/c"),
    ("DELETE", "/api/dev/apps/Miffy/eval_sets/e/evals/c"),
    ("POST", "/api/dev/apps/Miffy/eval_sets/e/add_session"),
    ("POST", "/api/dev/apps/Miffy/eval_sets/e/run_eval"),
    ("GET", "/api/dev/apps/Miffy/eval_results"),
    ("GET", "/api/dev/apps/Miffy/eval_results/r"),
    ("GET", "/api/dev/apps/Miffy/metrics-info"),
    ("GET", "/api/dev/apps/Miffy/debug/trace/ev"),
    ("GET", "/api/dev/apps/Miffy/debug/trace/session/s"),
    (
        "GET",
        "/api/dev/apps/Miffy/users/u/sessions/s/events/ev/graph",
    ),
    ("GET", "/api/dev/apps/Miffy/tests"),
    ("GET", "/api/dev/apps/Miffy/tests/t"),
    ("PUT", "/api/dev/apps/Miffy/tests/t"),
    ("DELETE", "/api/dev/apps/Miffy/tests/t"),
    ("POST", "/api/dev/apps/Miffy/tests/rebuild"),
    ("POST", "/api/dev/apps/Miffy/tests/run"),
    ("POST", "/api/run"),
    ("GET", "/api/health"),
    ("POST", "/api/apps/Miffy/users/u/sessions/s"),
    ("GET", "/api/apps/Miffy/users/u/sessions/s/artifacts"),
    ("DELETE", "/api/apps/Miffy/users/u/sessions/s/artifacts/a"),
    ("POST", "/api/dev/apps/Miffy/eval_sets"),
    ("GET", "/api/apps/Miffy/eval_sets"),
    ("GET", "/api/apps/Miffy/eval_results"),
    ("POST", "/api/apps/Miffy/eval_sets/x"),
    ("GET", "/api/debug/trace/ev"),
    ("GET", "/api/debug/trace/session/s"),
    ("GET", "/api/apps/Miffy/users/u/sessions/s/events/ev/graph"),
    ("OPTIONS", "/api/list-apps"),
    ("GET", "/health"),
    ("HEAD", "/health"),
    ("GET", "/"),
    ("GET", "/ui"),
    ("GET", "/ui/"),
    ("GET", "/ui/index.html"),
    ("GET", "/ui/adk_favicon.svg"),
    ("GET", "/ui/assets/config/runtime-config.json"),
    ("GET", "/.well-known/agent-card.json"),
    ("POST", "/a2a/invoke"),
    ("POST", "/a2a/v1/invoke"),
    ("POST", "/api/apps/Miffy/trigger/pubsub"),
    ("POST", "/api/apps/Miffy/trigger/eventarc"),
    ("GET", "/nope"),
];

#[test]
fn every_route_needs_the_token() {
    block_on(async {
        let w = web(ARGS, vec![]).await;
        let wrong = format!("Bearer {}x", TOKEN);
        let cookie = format!("youtube_token={}x", UI_TOKEN);
        let bearer_as_cookie = format!("youtube_token={TOKEN}");
        let ui_as_bearer = format!("Bearer {UI_TOKEN}");
        let refusals: [&[(&str, &str)]; 7] = [
            &[],
            &[("Authorization", &wrong)],
            &[("Authorization", TOKEN)],
            &[("Cookie", &cookie)],
            &[("Cookie", &bearer_as_cookie)],
            &[("Authorization", &ui_as_bearer)],
            &[("Authorization", "Basic dGVzdA==")],
        ];
        for (method, path) in ROUTES {
            for headers in refusals {
                let r = http(w.port, method, path, headers, b"").await;
                assert_eq!(r.status, 401, "{method} {path} with {headers:?}");
                assert_eq!(
                    r.header("www-authenticate"),
                    Some("Bearer realm=\"youtube\"")
                );
                if *method != "HEAD" {
                    assert_eq!(r.text(), "unauthorized\n");
                }
            }
            let bearer = format!("bearer {TOKEN}");
            let r = http(w.port, method, path, &[("Authorization", &bearer)], b"").await;
            assert_ne!(r.status, 401, "{method} {path} with the token");
        }
        assert!(w.youtube.lock().unwrap().is_empty());
    });
}

#[test]
fn the_ui_link_signs_the_browser_in_with_a_cookie() {
    block_on(async {
        let w = web(ARGS, vec![]).await;
        let link = format!("/ui/?token={UI_TOKEN}&mode=x");
        let r = http(w.port, "GET", &link, &[], b"").await;
        assert_eq!(r.status, 302);
        assert_eq!(r.header("location"), Some("/ui/?mode=x"));
        assert_eq!(
            r.header("set-cookie"),
            Some(format!("youtube_token={UI_TOKEN}; Path=/; HttpOnly; SameSite=Strict").as_str())
        );
        let cookie = format!("youtube_token={UI_TOKEN}");
        let r = http(w.port, "GET", "/api/list-apps", &[("Cookie", &cookie)], b"").await;
        assert_eq!((r.status, r.text().as_str()), (200, "[\"Miffy\"]\n"));
        let page = http(w.port, "GET", "/ui/", &[("Cookie", &cookie)], b"").await;
        assert_eq!(
            page.header("content-security-policy"),
            Some("img-src 'self' data: blob:")
        );
        let bad = http(w.port, "GET", "/ui/?token=nope", &[], b"").await;
        assert_eq!(bad.status, 401);
        let bearer_link = format!("/ui/?token={TOKEN}");
        let r = http(w.port, "GET", &bearer_link, &[], b"").await;
        assert_ne!(r.status, 302, "the bearer token does not sign a browser in");
        let not_ui = format!("/api/list-apps?token={TOKEN}");
        assert_eq!(http(w.port, "GET", &not_ui, &[], b"").await.status, 401);
    });
}

#[test]
fn other_local_pages_cannot_drive_any_route_with_the_cookie() {
    block_on(async {
        let w = web(ARGS, vec![]).await;
        let cookie = format!("youtube_token={UI_TOKEN}");
        for path in [
            "/a2a/invoke",
            "/a2a/v1/invoke",
            "/api/apps/Miffy/trigger/pubsub",
            "/api/apps/Miffy/trigger/eventarc",
            "/api/run_sse",
        ] {
            let headers = [
                ("Cookie", cookie.as_str()),
                ("Origin", "http://localhost:9999"),
                ("Content-Type", "application/json"),
            ];
            let r = http(w.port, "POST", path, &headers, b"{}").await;
            assert_eq!(r.status, 403, "{path}");
        }
        assert!(w.model.lock().unwrap().is_empty());
    });
}
