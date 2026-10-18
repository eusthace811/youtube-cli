//! ADK's web launcher: one HTTP server on `-host:-port` (loopback by
//! default) whose routes the active sublaunchers mount, in ADK's order.
//! Every route needs the bearer token printed at launch (divergence E11);
//! the dev UI gets it as a cookie from its first, token-carrying URL.

use ::http::Method;
use std::sync::Arc;
use tokio::net::TcpListener;

use super::a2a::{self, Tasks};
use super::http::{self, Req, Resp};
use super::session::Store;
use super::{api, triggers, webui};
use crate::agent::Agent;
use crate::launcher::WebConfig;

const COOKIE: &str = "yutu_token";

pub struct App {
    pub agent: Arc<Agent>,
    pub cfg: WebConfig,
    pub store: Store,
    pub tasks: Tasks,
    pub eventarc_slots: Option<tokio::sync::Semaphore>,
    token: String,
    card: Option<String>,
}

/// How a request proved it holds the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller {
    /// `Authorization: Bearer`: a program, with no human to approve writes.
    Bearer,
    /// The dev UI's cookie: a person at the browser.
    Browser,
}

impl App {
    /// `instruction` and `google_search` describe the agent on the A2A card.
    pub fn new(
        agent: Arc<Agent>,
        cfg: WebConfig,
        token: String,
        instruction: &str,
        google_search: bool,
    ) -> App {
        let card = cfg
            .a2a
            .as_ref()
            .map(|a| a2a::card(&a.agent_url, instruction, google_search));
        let eventarc_slots = cfg.eventarc.as_ref().map(|e| {
            tokio::sync::Semaphore::new(usize::try_from(e.max_concurrent_runs).unwrap_or(1))
        });
        App {
            agent,
            cfg,
            store: Store::default(),
            tasks: Tasks::default(),
            eventarc_slots,
            token,
            card,
        }
    }

    fn caller(&self, req: &Req) -> Option<Caller> {
        let bearer = req.header("authorization").and_then(|h| {
            let (scheme, value) = h.split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then(|| value.trim())
        });
        if bearer.is_some_and(|t| same(t, &self.token)) {
            return Some(Caller::Bearer);
        }
        let cookie = req.headers.get_all("cookie").iter().find_map(|v| {
            v.to_str().ok()?.split(';').find_map(|pair| {
                let (k, v) = pair.trim().split_once('=')?;
                (k == COOKIE && same(v, &self.token)).then_some(())
            })
        });
        cookie.map(|()| Caller::Browser)
    }
}

/// Compares in time independent of where the strings differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn unauthorized() -> Resp {
    Resp::error(401, "unauthorized").header("www-authenticate", "Bearer realm=\"yutu\"")
}

/// Matches `/{prefix}/apps/{app_name}/trigger/{kind}`; the app name.
fn trigger_app<'a>(path: &'a str, prefix: &str, kind: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(prefix)?.strip_prefix("/apps/")?;
    let (app, tail) = rest.split_once('/')?;
    (tail == format!("trigger/{kind}") && !app.is_empty()).then_some(app)
}

pub async fn handle(app: Arc<App>, req: Req) -> Resp {
    let Some(caller) = app.caller(&req) else {
        return sign_in(&app, &req).unwrap_or_else(unauthorized);
    };
    let path = req.path.clone();
    let get = matches!(req.method.as_str(), "GET" | "HEAD");
    let mut wrong_method = false;
    if path == "/health" {
        if get {
            return Resp::new(200)
                .header("content-type", "application/json")
                .body("{\"status\":\"ok\"}\n");
        }
        wrong_method = true;
    }
    if let Some(ui) = &app.cfg.webui
        && let Some(resp) = webui::handle(ui, &req)
    {
        if resp.status != 405 {
            return resp;
        }
        wrong_method = true;
    }
    if let Some(card) = &app.card {
        match path.as_str() {
            "/.well-known/agent-card.json" => return a2a::card_response(card),
            "/a2a/v1/invoke" => return a2a::handle(&app, &req, a2a::Version::V1).await,
            "/a2a/invoke" => return a2a::handle(&app, &req, a2a::Version::V0).await,
            _ => {}
        }
    }
    let post = req.method == Method::POST;
    if let Some(cfg) = &app.cfg.pubsub
        && let Some(name) = trigger_app(&path, &cfg.path_prefix, "pubsub")
    {
        if post {
            return triggers::pubsub(&app, cfg, &req, name).await;
        }
        wrong_method = true;
    }
    if let Some(cfg) = &app.cfg.eventarc
        && let Some(name) = trigger_app(&path, &cfg.path_prefix, "eventarc")
    {
        if post {
            return triggers::eventarc(&app, cfg, &req, name).await;
        }
        wrong_method = true;
    }
    if let Some(cfg) = &app.cfg.api {
        let below = if cfg.path_prefix.is_empty() {
            Some(path.as_str())
        } else if path == cfg.path_prefix {
            Some("/")
        } else {
            path.strip_prefix(&cfg.path_prefix)
                .filter(|p| p.starts_with('/'))
        };
        if let Some(below) = below {
            let human = caller == Caller::Browser && app.cfg.webui.is_some();
            let below = below.to_string();
            return api::handle(&app, cfg, req, &below, human).await;
        }
    }
    if wrong_method {
        Resp::new(405)
    } else {
        Resp::not_found()
    }
}

/// The dev UI's sign-in: a GET under `/ui` carrying `?token=` sets the
/// token cookie and reloads the page without it, so the token leaves the
/// address bar and every later request from the page carries the cookie.
fn sign_in(app: &App, req: &Req) -> Option<Resp> {
    app.cfg.webui.as_ref()?;
    let under_ui = req.path == "/ui" || req.path.starts_with(webui::PREFIX);
    if req.method != Method::GET || !under_ui {
        return None;
    }
    let token = req.param("token")?;
    if !same(&token, &app.token) {
        return None;
    }
    let rest: Vec<&str> = req
        .query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("token="))
        .collect();
    let mut location = req.path.clone();
    if !rest.is_empty() {
        location = format!("{location}?{}", rest.join("&"));
    }
    Some(
        Resp::redirect(302, &location)
            .header(
                "set-cookie",
                format!("{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict", app.token),
            )
            .header("cache-control", "no-store"),
    )
}

fn info(msg: &str) {
    yutu_auth::log::info(msg, &[]);
}

/// ADK's `webLauncher.Run`: the startup lines, then serve until Ctrl-C.
pub async fn run(app: Arc<App>) -> Result<(), String> {
    let cfg = &app.cfg;
    info(
        "No artifact service configured. Using an in-memory one, so whatever it holds is lost when the process exits.",
    );
    if cfg.webui.is_none()
        && cfg.a2a.is_none()
        && cfg.pubsub.is_none()
        && cfg.eventarc.is_none()
        && cfg.api.is_none()
    {
        return Err("no active sublaunchers found - please specify them in the command line. Possible values: [webui a2a pubsub eventarc api]".into());
    }
    if cfg.otel_to_cloud {
        yutu_auth::log::warn("telemetry is not supported in yutu-rs", &[]);
    }
    if cfg.h2c {
        yutu_auth::log::warn("-h2c is not supported in yutu-rs; serving HTTP/1.1", &[]);
    }
    info(&format!(
        "Starting the web server: &{{port:{} host:{} writeTimeout:{} readTimeout:{} idleTimeout:{} shutdownTimeout:{} otelToCloud:{} useH2C:{} maxPayloadSize:{}}}",
        cfg.port,
        cfg.host,
        cfg.write_timeout_ns,
        cfg.read_timeout_ns,
        cfg.idle_timeout_ns,
        cfg.shutdown_timeout_ns,
        cfg.otel_to_cloud,
        cfg.h2c,
        cfg.max_request_body_size
    ));
    info("");
    let url = cfg.web_url();
    info(&format!("Web servers starts on {url}"));
    info(&format!(
        "       token: {}  (send it as \"Authorization: Bearer <token>\"; every route requires it)",
        app.token
    ));
    if cfg.webui.is_some() {
        info(&format!(
            "       webui:  you can access API using {url}{}",
            webui::PREFIX
        ));
        info(&format!(
            "       webui:  sign in the browser with {url}{}?token={}",
            webui::PREFIX,
            app.token
        ));
    }
    if cfg.a2a.is_some() {
        info(&format!(
            "       a2a:  you can access A2A using jsonrpc protocol: {url}"
        ));
    }
    if let Some(p) = &cfg.pubsub {
        info(&format!(
            "       pubsub:  PubSub trigger endpoint is available at {url}{}/apps/{{app_name}}/trigger/pubsub",
            p.path_prefix
        ));
    }
    if let Some(e) = &cfg.eventarc {
        info(&format!(
            "       eventarc: Eventarc trigger endpoint is available at {url}{}/apps/{{app_name}}/trigger/eventarc",
            e.path_prefix
        ));
    }
    if let Some(a) = &cfg.api {
        info(&format!(
            "       api:  you can access API using {url}{}",
            a.path_prefix
        ));
        info(&format!(
            "       api:      for instance: {url}{}/list-apps",
            a.path_prefix
        ));
        if !a.include_debug_api {
            info(
                "       api:      debug excluded: for the web UI's Traces and agent structure panels, use -include_debug_api",
            );
        }
    }
    info("");
    let listener = bind(cfg).await?;
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
        info("Shutting down the web server...");
    };
    serve(app, listener, shutdown).await;
    Ok(())
}

/// Serves `app` on `listener` until `shutdown` resolves.
pub async fn serve(
    app: Arc<App>,
    listener: TcpListener,
    shutdown: impl std::future::Future<Output = ()>,
) {
    let cfg = &app.cfg;
    let limits = http::Limits {
        read: cfg.read_timeout(),
        write: cfg.write_timeout(),
        idle: cfg.idle_timeout(),
        body: cfg.body_cap(),
    };
    let grace = cfg.shutdown_timeout();
    let handler_app = app.clone();
    http::serve(
        listener,
        limits,
        move |req| handle(handler_app.clone(), req),
        shutdown,
        grace,
    )
    .await;
}

/// `net.Listen("tcp", host:port)` with Go's error texts for the common
/// failures.
pub async fn bind(cfg: &WebConfig) -> Result<TcpListener, String> {
    let host = cfg.bind_host();
    let port = cfg.port;
    let joined = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    if !(0..=65535).contains(&port) {
        return Err(format!(
            "server failed: listen tcp: address {joined}: invalid port"
        ));
    }
    TcpListener::bind(&joined).await.map_err(|e| {
        let why = match e.kind() {
            std::io::ErrorKind::AddrInUse => "bind: address already in use".to_string(),
            std::io::ErrorKind::PermissionDenied => "bind: permission denied".to_string(),
            std::io::ErrorKind::AddrNotAvailable => {
                "bind: can't assign requested address".to_string()
            }
            _ => e.to_string(),
        };
        format!("server failed: listen tcp {joined}: {why}")
    })
}
