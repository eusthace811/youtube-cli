//! ADK's REST API (`server/adkrest`) as the `api` sublauncher mounts it:
//! CORS for the web UI's origin, the origin and Host checks, then the
//! routes the dev UI calls, with Go's JSON shapes, texts and status codes.

use bytes::Bytes;
use http::Method;
use serde_json::{Map, Value, json};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

use super::http::{Req, Resp};
use super::session::{Input, Session};
use super::web::App;
use super::wire;
use crate::agent;
use crate::launcher::ApiConfig;

const CORS_METHODS: &str = "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS";
const PROBE: [Method; 7] = [
    Method::GET,
    Method::HEAD,
    Method::POST,
    Method::PUT,
    Method::PATCH,
    Method::DELETE,
    Method::OPTIONS,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Health,
    GetSession,
    CreateSession,
    UpdateSession,
    DeleteSession,
    ListSessions,
    Run,
    RunSse,
    RunLive,
    ListApps,
    ListArtifacts,
    LoadArtifact,
    LoadArtifactVersion,
    DeleteArtifact,
    Version,
    Builder,
    BuilderNotImplemented,
    TestsNotImplemented,
    MetricsInfo,
    EvalNotImplemented,
    SessionTrace,
    EventTrace,
    EventGraph,
    BuildGraph,
    BuildGraphImage,
}

const SESSION: &str = "/apps/{app_name}/users/{user_id}/sessions/{session_id}";
const DEV: &str = "/dev/apps/{app_name}";

/// The REST router's table in ADK's registration order; GET routes also
/// answer HEAD.
fn routes(debug: bool) -> Vec<(Method, String, Route)> {
    use Method as M;
    use Route::*;
    let s = |p: &str| format!("{SESSION}{p}");
    let d = |p: &str| format!("{DEV}{p}");
    let mut r = vec![
        (M::GET, "/health".into(), Health),
        (M::GET, s(""), GetSession),
        (
            M::POST,
            "/apps/{app_name}/users/{user_id}/sessions".into(),
            CreateSession,
        ),
        (M::POST, s(""), CreateSession),
        (M::PATCH, s(""), UpdateSession),
        (M::DELETE, s(""), DeleteSession),
        (
            M::GET,
            "/apps/{app_name}/users/{user_id}/sessions".into(),
            ListSessions,
        ),
        (M::POST, "/run".into(), Run),
        (M::POST, "/run_sse".into(), RunSse),
        (M::GET, "/run_live".into(), RunLive),
        (M::GET, "/list-apps".into(), ListApps),
        (M::GET, s("/artifacts"), ListArtifacts),
        (M::GET, s("/artifacts/{artifact_name}"), LoadArtifact),
        (
            M::GET,
            s("/artifacts/{artifact_name}/versions/{version}"),
            LoadArtifactVersion,
        ),
        (M::DELETE, s("/artifacts/{artifact_name}"), DeleteArtifact),
        (M::GET, "/version".into(), Version),
        (M::GET, d("/builder"), Builder),
        (M::POST, d("/builder/save"), BuilderNotImplemented),
        (M::POST, d("/builder/cancel"), BuilderNotImplemented),
    ];
    for (m, p) in [
        (M::GET, "/tests"),
        (M::POST, "/tests/rebuild"),
        (M::POST, "/tests/run"),
        (M::GET, "/tests/{test_name}"),
        (M::PUT, "/tests/{test_name}"),
        (M::DELETE, "/tests/{test_name}"),
    ] {
        r.push((m, d(p), TestsNotImplemented));
    }
    r.push((M::GET, d("/metrics-info"), MetricsInfo));
    for (m, p) in [
        (M::GET, "/eval_sets"),
        (M::POST, "/eval-sets"),
        (M::POST, "/eval_sets"),
        (M::GET, "/eval_sets/{eval_set_id}"),
        (M::DELETE, "/eval_sets/{eval_set_id}"),
        (M::GET, "/eval_sets/{eval_set_id}/evals"),
        (M::GET, "/eval_sets/{eval_set_id}/evals/{eval_case_id}"),
        (M::PUT, "/eval_sets/{eval_set_id}/evals/{eval_case_id}"),
        (M::DELETE, "/eval_sets/{eval_set_id}/evals/{eval_case_id}"),
        (M::POST, "/eval_sets/{eval_set_id}/add_session"),
        (M::POST, "/eval_sets/{eval_set_id}/run_eval"),
        (M::GET, "/eval_results"),
        (M::GET, "/eval_results/{eval_result_id}"),
    ] {
        r.push((m, d(p), EvalNotImplemented));
    }
    for (m, p) in [
        (M::GET, "/apps/{app_name}/eval_sets"),
        (M::GET, "/apps/{app_name}/eval_results"),
        (M::POST, "/apps/{app_name}/eval_sets/{eval_set_name}"),
    ] {
        r.push((m, p.into(), EvalNotImplemented));
    }
    if debug {
        r.extend([
            (M::GET, d("/debug/trace/session/{session_id}"), SessionTrace),
            (M::GET, d("/debug/trace/{event_id}"), EventTrace),
            (
                M::GET,
                d("/users/{user_id}/sessions/{session_id}/events/{event_id}/graph"),
                EventGraph,
            ),
            (M::GET, "/debug/trace/{event_id}".into(), EventTrace),
            (M::GET, s("/events/{event_id}/graph"), EventGraph),
            (
                M::GET,
                "/debug/trace/session/{session_id}".into(),
                SessionTrace,
            ),
            (M::GET, d("/build_graph"), BuildGraph),
            (M::GET, d("/build_graph_image"), BuildGraphImage),
        ]);
    }
    r
}

/// Path variables when `path` matches `pattern`; each `{var}` takes one
/// non-empty segment, as gorilla/mux's default does.
fn matches(pattern: &str, path: &str) -> Option<Map<String, Value>> {
    let (p, s): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
    if p.len() != s.len() {
        return None;
    }
    let mut vars = Map::new();
    for (p, s) in p.iter().zip(&s) {
        if let Some(name) = p.strip_prefix('{').and_then(|n| n.strip_suffix('}')) {
            if s.is_empty() {
                return None;
            }
            vars.insert(name.into(), (*s).into());
        } else if p != s {
            return None;
        }
    }
    Some(vars)
}

fn allows(route: &Method, method: &Method) -> bool {
    route == method || (*route == Method::GET && *method == Method::HEAD)
}

/// The api mount: `path` is the request path below the prefix.
pub async fn handle(app: &Arc<App>, cfg: &ApiConfig, req: Req, path: &str, human: bool) -> Resp {
    let origin = super::origin::normalize(&cfg.webui_address);
    let cors = |r: Resp| {
        let mut r = r;
        if !origin.is_empty() {
            r = r.header("access-control-allow-origin", origin.clone());
            if origin != "*" {
                r = r.header("vary", "Origin");
            }
        }
        r.header("access-control-allow-methods", CORS_METHODS)
            .header(
                "access-control-allow-headers",
                "Content-Type, Authorization",
            )
    };
    if req.method == Method::OPTIONS {
        return cors(Resp::new(200));
    }
    if let Err(e) = super::origin::check(&req, &cfg.webui_address, app.cfg.bind_host()) {
        return cors(Resp::error(403, &format!("Forbidden: {e}")));
    }
    let trimmed = path.trim_end_matches('/');
    let path = if trimmed.is_empty() { "/" } else { trimmed };
    let table = routes(cfg.include_debug_api);
    let found = table
        .iter()
        .find_map(|(m, p, r)| allows(m, &req.method).then(|| matches(p, path).map(|v| (*r, v)))?);
    let Some((route, vars)) = found else {
        let allowed: Vec<&str> = PROBE
            .iter()
            .filter(|m| {
                table
                    .iter()
                    .any(|(rm, p, _)| allows(rm, m) && matches(p, path).is_some())
            })
            .map(Method::as_str)
            .collect();
        if allowed.is_empty() {
            return cors(Resp::not_found());
        }
        return cors(Resp::new(405).header("allow", allowed.join(", ")));
    };
    cors(dispatch(app, cfg, &req, route, &vars, human).await)
}

async fn dispatch(
    app: &Arc<App>,
    cfg: &ApiConfig,
    req: &Req,
    route: Route,
    vars: &Map<String, Value>,
    human: bool,
) -> Resp {
    use Route::*;
    let var = |k: &str| {
        vars.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let (a, u, s) = (var("app_name"), var("user_id"), var("session_id"));
    match route {
        Health => Resp::new(200)
            .header("content-type", "application/json")
            .body("{\"status\":\"ok\"}\n"),
        ListApps => Resp::json(200, &json!([agent::NAME])),
        Version => Resp::json(
            200,
            &json!({"version": "2.5.0", "language": "rust",
                    "language_version": env!("YUTU_RUSTC_VERSION")}),
        ),
        CreateSession => create_session(app, req, &a, &u, &s),
        GetSession => match app.store.get(&a, &u, &s) {
            Ok(sess) => Resp::json(200, &sess.json(true)),
            Err(e) => Resp::error(404, &e),
        },
        UpdateSession => update_session(app, req, &a, &u, &s),
        DeleteSession => {
            app.store.delete(&a, &u, &s);
            Resp::new(200).header("content-type", "application/json; charset=UTF-8")
        }
        ListSessions => {
            let list: Vec<Value> = app
                .store
                .list(&a, &u)
                .iter()
                .map(|s| s.json(false))
                .collect();
            Resp::json(200, &Value::Array(list))
        }
        Run => run(app, req, human).await,
        RunSse => run_sse(app, cfg, req, human).await,
        RunLive => run_live(req),
        ListArtifacts => Resp::json(200, &json!([])),
        LoadArtifact => Resp::error(404, "artifact not found: file does not exist"),
        LoadArtifactVersion => {
            if var("version").parse::<i64>().is_err() {
                Resp::error(400, "version parameter must be an integer")
            } else {
                Resp::error(404, "artifact not found: file does not exist")
            }
        }
        DeleteArtifact => Resp::new(200).header("content-type", "application/json; charset=UTF-8"),
        Builder => Resp::new(200).header("content-type", "text/plain; charset=UTF-8"),
        BuilderNotImplemented => not_implemented(
            "the agent builder",
            "ADK Go agents are defined in Go code, so there is no server-side agent configuration to edit.",
        ),
        TestsNotImplemented => not_implemented(
            "the tests tab",
            "ADK Go does not implement the web UI tests tab. Write Go tests against runner.Run instead.",
        ),
        EvalNotImplemented => not_implemented(
            "eval",
            "agent evaluation is not installed in ADK Go. Use adk-python for eval workflows.",
        ),
        MetricsInfo => Resp::json(200, &json!({"metricsInfo": []})),
        // Traces are OpenTelemetry spans in Go; yutu-rs records none (E12).
        SessionTrace => Resp::json(200, &json!([])),
        EventTrace => Resp::error(404, &format!("event not found: {}", var("event_id"))),
        EventGraph => match app.store.get(&a, &u, &s) {
            Ok(sess) if sess.has_event(&var("event_id")) => Resp::json(
                200,
                &json!({"dotSrc": graph("#333537", "#0F5223", "#cccccc", "filled")}),
            ),
            _ => Resp::error(404, "event not found"),
        },
        BuildGraph => Resp::json(
            200,
            &json!({"root_agent": {"name": agent::NAME, "description": agent::DESCRIPTION,
                    "sub_agents": []}, "readme": ""}),
        ),
        BuildGraphImage => Resp::json(
            200,
            &json!({ agent::NAME: {"dotSrc": graph("#ffffff", "#3c4043", "#3c4043", "rounded")} }),
        ),
    }
}

/// The DOT graph ADK's debug API draws for a lone LLM agent.
fn graph(background: &str, color: &str, font: &str, style: &str) -> String {
    format!(
        "digraph AgentGraph {{\n\tbgcolor=\"{background}\";\n\trankdir=LR;\n\t{0} [ color=\"{color}\", fontcolor=\"{font}\", label=\"🤖 {0}\", shape=ellipse, style={style} ];\n\n}}\n",
        agent::NAME
    )
}

fn not_implemented(feature: &str, detail: &str) -> Resp {
    Resp::json(
        501,
        &json!({"error": format!("{feature} is not implemented in ADK Go"), "detail": detail}),
    )
}

/// Decodes an optional JSON body as Go's handlers do: an empty one is no
/// body at all.
fn optional_body(req: &Req) -> Result<Value, String> {
    let body = req.body().map_err(String::from)?;
    match wire::decode(body) {
        Ok(v) => Ok(v),
        Err(e) if e == "EOF" => Ok(Value::Null),
        Err(e) => Err(e),
    }
}

fn create_session(app: &Arc<App>, req: &Req, a: &str, u: &str, s: &str) -> Resp {
    let body = match optional_body(req) {
        Ok(v) => v,
        Err(e) => return Resp::error(400, &e),
    };
    let state = body["state"].as_object().cloned().unwrap_or_default();
    match app.store.create(a, u, Some(s), state) {
        Ok(sess) => Resp::json(200, &sess.json(true)),
        Err(e) => Resp::error(500, &e),
    }
}

fn update_session(app: &Arc<App>, req: &Req, a: &str, u: &str, s: &str) -> Resp {
    let body = match optional_body(req) {
        Ok(v) => v,
        Err(e) => return Resp::error(400, &e),
    };
    let sess = match app.store.get(a, u, s) {
        Ok(sess) => sess,
        Err(e) => return Resp::error(404, &e),
    };
    let stored = sess.state();
    let scoped =
        |k: &str| k.starts_with("app:") || k.starts_with("user:") || k.starts_with("temp:");
    let delta: Map<String, Value> = body["stateDelta"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, v)| !(scoped(k) && stored.get(*k) == Some(*v)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut bad: Vec<&String> = delta.keys().filter(|k| scoped(k)).collect();
    bad.sort();
    if !bad.is_empty() {
        let list: Vec<&str> = bad.iter().map(|s| s.as_str()).collect();
        return Resp::error(
            400,
            &format!(
                "stateDelta may only contain session-scoped keys; remove [{}] (the \"app:\", \"user:\" and \"temp:\" prefixes write outside this session)",
                list.join(" ")
            ),
        );
    }
    if !delta.is_empty() {
        sess.update_state(delta);
    }
    Resp::json(200, &sess.json(true))
}

/// ADK's `RunAgentRequest`, decoded with `DisallowUnknownFields` and Go's
/// case-insensitive field names.
struct RunRequest {
    app: String,
    user: String,
    session: String,
    message: Value,
    streaming: bool,
    state_delta: Option<Map<String, Value>>,
}

fn decode_run(req: &Req) -> Result<RunRequest, String> {
    let fail = |e: String| format!("failed to decode request: {e}");
    let v = wire::decode(req.body().map_err(|e| fail(e.into()))?).map_err(fail)?;
    let Value::Object(m) = v else {
        let kind = match v {
            Value::Array(_) => "array",
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Bool(_) => "bool",
            _ => "null",
        };
        if kind == "null" {
            return Ok(RunRequest {
                app: String::new(),
                user: String::new(),
                session: String::new(),
                message: Value::Null,
                streaming: false,
                state_delta: None,
            });
        }
        return Err(fail(format!(
            "json: cannot unmarshal {kind} into Go value of type models.RunAgentRequest"
        )));
    };
    const FIELDS: [&str; 7] = [
        "appName",
        "userId",
        "sessionId",
        "newMessage",
        "streaming",
        "stateDelta",
        "functionCallEventId",
    ];
    let mut r = RunRequest {
        app: String::new(),
        user: String::new(),
        session: String::new(),
        message: Value::Null,
        streaming: false,
        state_delta: None,
    };
    for (k, v) in m {
        let Some(field) = FIELDS.iter().find(|f| f.eq_ignore_ascii_case(&k)) else {
            return Err(fail(format!(
                "json: unknown field {}",
                yutu_auth::log::quote(&k)
            )));
        };
        let type_err = |want: &str| {
            fail(format!(
                "json: cannot unmarshal {} into Go struct field RunAgentRequest.{field} of type {want}",
                match v {
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                    Value::String(_) => "string",
                    Value::Number(_) => "number",
                    Value::Bool(_) => "bool",
                    Value::Null => "null",
                }
            ))
        };
        let string = |v: &Value| match v {
            Value::String(s) => Ok(s.clone()),
            Value::Null => Ok(String::new()),
            _ => Err(type_err("string")),
        };
        match *field {
            "appName" => r.app = string(&v)?,
            "userId" => r.user = string(&v)?,
            "sessionId" => r.session = string(&v)?,
            "functionCallEventId" => {
                string(&v)?;
            }
            "streaming" => {
                r.streaming = match v {
                    Value::Bool(b) => b,
                    Value::Null => false,
                    _ => return Err(type_err("bool")),
                }
            }
            "stateDelta" => {
                r.state_delta = match v {
                    Value::Object(o) => Some(o),
                    Value::Null => None,
                    _ => return Err(type_err("map[string]interface {}")),
                }
            }
            _ => {
                if !matches!(v, Value::Object(_) | Value::Null) {
                    return Err(type_err("genai.Content"));
                }
                r.message = v;
            }
        }
    }
    Ok(r)
}

/// ADK's agent loader for this app: only Miffy, or the empty name.
fn load_agent(name: &str) -> Result<(), String> {
    if name.is_empty() || name == agent::NAME {
        Ok(())
    } else {
        Err(format!(
            "failed to load agent: cannot load agent '{name}' - provide an empty string or use '{}'",
            agent::NAME
        ))
    }
}

fn find_session(app: &Arc<App>, r: &RunRequest) -> Result<Arc<Session>, String> {
    app.store
        .get(&r.app, &r.user, &r.session)
        .map_err(|e| format!("failed to get session: {e}"))
}

async fn run(app: &Arc<App>, req: &Req, human: bool) -> Resp {
    let r = match decode_run(req) {
        Ok(r) => r,
        Err(e) => return Resp::error(400, &e),
    };
    let sess = match find_session(app, &r) {
        Ok(s) => s,
        Err(e) => return Resp::error(404, &e),
    };
    if let Err(e) = load_agent(&r.app) {
        return Resp::error(500, &e);
    }
    let mut events = Vec::new();
    let input = Input {
        message: r.message,
        state_delta: r.state_delta,
        stream: r.streaming,
        human,
    };
    let result = sess
        .run(&app.agent, input, &mut |e| {
            if e.get("partial").is_none() {
                events.push(e);
            }
        })
        .await;
    match result {
        Err(e) => Resp::error(500, &format!("failed to run agent: {e}")),
        Ok(()) if events.is_empty() => Resp::json(200, &Value::Null),
        Ok(()) => Resp::json(200, &Value::Array(events)),
    }
}

async fn run_sse(app: &Arc<App>, cfg: &ApiConfig, req: &Req, human: bool) -> Resp {
    let deadline = cfg.sse_write_timeout.map(|t| Instant::now() + t);
    let r = match decode_run(req) {
        Ok(r) => r,
        Err(e) => return Resp::error(400, &format!("failed to decode request body: {e}")),
    };
    let sess = match find_session(app, &r) {
        Ok(s) => s,
        Err(e) => return Resp::error(404, &format!("failed to find the session: {e}")),
    };
    if let Err(e) = load_agent(&r.app) {
        return Resp::error(500, &format!("failed to get runner: {e}"));
    }
    let (tx, rx) = mpsc::unbounded_channel::<Bytes>();
    let agent = app.agent.clone();
    let input = Input {
        message: r.message,
        state_delta: r.state_delta,
        stream: r.streaming,
        human,
    };
    tokio::spawn(async move {
        let send = tx.clone();
        let result = sess
            .run(&agent, input, &mut |e| {
                let _ = send.send(format!("data: {}\n\n", crate::gojson::encode(&e)).into());
            })
            .await;
        if let Err(e) = result {
            let data = crate::gojson::encode(&json!({ "error": e }));
            let _ = tx.send(format!("event: error\ndata: {data}\n\n").into());
        }
    });
    Resp::stream(200, rx, deadline)
        .header("content-type", "text/event-stream; charset=UTF-8")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
}

/// `/run_live` needs Gemini's live (WebSocket audio) API, which yutu-rs
/// does not speak. A plain GET gets gorilla/websocket's refusal, as in Go;
/// a real upgrade is refused with 501.
fn run_live(req: &Req) -> Resp {
    let first = |a: &str, b: &str| {
        req.param(a)
            .filter(|v| !v.is_empty())
            .or_else(|| req.param(b))
    };
    let (app, user, session) = (
        first("appName", "app_name").unwrap_or_default(),
        first("userId", "user_id").unwrap_or_default(),
        first("sessionId", "session_id").unwrap_or_default(),
    );
    if app.is_empty() || user.is_empty() || session.is_empty() {
        return Resp::error(500, "appName, userId, and sessionId are required");
    }
    let upgrade = req.header("connection").is_some_and(|c| {
        c.split(',')
            .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
    });
    if upgrade {
        return Resp::error(
            501,
            "live sessions (/run_live) are not supported in yutu-rs",
        );
    }
    Resp::new(400)
        .header("content-type", "text/plain; charset=utf-8")
        .header("x-content-type-options", "nosniff")
        .body("Bad Request\nfailed to upgrade to websocket: websocket: the client is not using the websocket protocol: 'upgrade' token not found in 'Connection' header\n")
}
