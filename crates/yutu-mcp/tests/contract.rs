//! The MCP contract, listed in-process through the dispatcher, against the
//! lists recorded from the Go server.

use serde_json::{Value, json};
use std::sync::Arc;
use yutu_api::Service;
use yutu_api::client::BoxFuture;
use yutu_core::ops::ServiceProvider;
use yutu_mcp::{Incoming, Server, Session};

struct NoService;

impl ServiceProvider for NoService {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        Box::pin(async { Err("listing needs no service".to_string()) })
    }
}

fn golden() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/golden/mcp-contract.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

async fn ask(s: &Session, frame: Value) -> Value {
    let Incoming::Single(msg) = Incoming::parse(&frame.to_string()).unwrap() else {
        panic!("not a single message");
    };
    serde_json::from_str(&s.handle(msg).await.expect("a reply")).unwrap()
}

async fn session() -> Arc<Session> {
    let s = Session::new(Server::new(Arc::new(NoService), "v0.11.0"), None);
    ask(
        &s,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "contract", "version": "0"}}}),
    )
    .await;
    s
}

fn list(id: u64, method: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": {}})
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn lists_74_tools_with_the_go_video_list_schema() {
    block_on(async {
        let s = session().await;
        let reply = ask(&s, list(2, "tools/list")).await;
        let tools = reply["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 74);
        let schema = |tools: &[Value]| {
            tools
                .iter()
                .find(|t| t["name"] == "video-list")
                .map(|t| t["inputSchema"].clone())
                .unwrap()
        };
        let want = golden();
        let want_tools = want["tools"]["result"]["tools"].as_array().unwrap();
        assert_eq!(schema(tools), schema(want_tools));
    })
}

#[test]
fn every_list_equals_the_go_contract() {
    block_on(async {
        let s = session().await;
        let want = golden();
        for (i, (key, method)) in [
            ("tools", "tools/list"),
            ("resources", "resources/list"),
            ("templates", "resources/templates/list"),
            ("prompts", "prompts/list"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = i as u64 + 2;
            let mut got = ask(&s, list(id, method)).await;
            let mut expected = want[key].clone();
            got["id"] = json!(0);
            expected["id"] = json!(0);
            assert_eq!(got, expected, "{method}");
        }
    })
}

#[test]
fn initialize_equals_the_go_contract() {
    block_on(async {
        let s = Session::new(Server::new(Arc::new(NoService), "v0.11.0"), None);
        let reply = ask(
            &s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "contract", "version": "0"}}}),
        )
        .await;
        assert_eq!(reply["result"], golden()["initialize"]);
    })
}
