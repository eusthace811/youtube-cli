//! ADK's `full` launcher argument handling: `--args` split like Go's
//! `strings.Fields`, routed by its first word to `console` or `web` and the
//! web sublaunchers, each flag set parsed with the Go `flag` package's rules
//! and messages.

use std::io::Write;
use std::time::Duration;

use crate::launch::flags::{Flag, FlagSet, Kind, Values};

/// What `yutu agent` prints for an empty `--args` or a launch failure:
/// the full launcher's `CommandLineSyntax()` plus Println's newline.
pub const SYNTAX: &str = include_str!("../assets/launcher-syntax.txt");

const fn flag(name: &'static str, kind: Kind, default: &'static str, usage: &'static str) -> Flag {
    Flag {
        name,
        kind,
        default,
        usage,
    }
}

const OTEL: &str = "Enables/disables OpenTelemetry export to GCP: telemetry.googleapis.com. See adk-go/telemetry package for details about supported options, credentials and environment variables.";

pub const CONSOLE: FlagSet = FlagSet {
    name: "console",
    flags: &[
        flag("otel_to_cloud", Kind::Bool, "false", OTEL),
        flag(
            "shutdown-timeout",
            Kind::Duration,
            "2s",
            "Console shutdown timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for waiting for active requests to finish during shutdown",
        ),
        flag(
            "streaming_mode",
            Kind::Str,
            "",
            "defines streaming mode (none|sse)",
        ),
    ],
};

pub const WEB: FlagSet = FlagSet {
    name: "web",
    flags: &[
        flag(
            "host",
            Kind::Str,
            "127.0.0.1",
            "Host/IP to bind the web server to. Defaults to 127.0.0.1 (loopback only) so the server is not exposed to the network. Use 0.0.0.0 to listen on all interfaces, which may be required when running adk web inside a container. An empty value is treated as the default.",
        ),
        flag("port", Kind::Int, "8080", "Port for the web server"),
        flag(
            "write-timeout",
            Kind::Duration,
            "15s",
            "Server write timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for writing the response after reading the headers & body",
        ),
        flag(
            "read-timeout",
            Kind::Duration,
            "15s",
            "Server read timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for reading the whole request including body",
        ),
        flag(
            "idle-timeout",
            Kind::Duration,
            "1m0s",
            "Server idle timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for waiting for the next request (only when keep-alive is enabled)",
        ),
        flag(
            "shutdown-timeout",
            Kind::Duration,
            "15s",
            "Server shutdown timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for waiting for active requests to finish during shutdown",
        ),
        flag("otel_to_cloud", Kind::Bool, "false", OTEL),
        flag(
            "h2c",
            Kind::Bool,
            "false",
            "Enable prior-knowledge cleartext HTTP/2 (h2c; no HTTP/1.1 Upgrade) on the web server listener. Cleartext is insecure; do not expose it to untrusted networks. Long-lived streaming responses may require increasing --write-timeout.",
        ),
        flag(
            "max_request_body_size",
            Kind::Int,
            "0",
            "Maximum request body size in bytes. Larger requests are rejected with HTTP 400. 0 uses the default (10 MiB) or a value set by an embedder. Applied to the base router so all sublauncher and trigger routes are covered.",
        ),
    ],
};

const WEBUI: FlagSet = FlagSet {
    name: "webui",
    flags: &[flag(
        "api_server_address",
        Kind::Str,
        "http://localhost:8080/api",
        "ADK REST API server address as seen from the user browser. Please specify the whole URL, i.e. 'http://localhost:8080/api'.",
    )],
};

const A2A: FlagSet = FlagSet {
    name: "a2a",
    flags: &[flag(
        "a2a_agent_url",
        Kind::Str,
        "http://localhost:8080",
        "A2A host URL as advertised in the public agent card. It is used by A2A clients as a connection endpoint.",
    )],
};

const fn trigger_flags(prefix_usage: &'static str) -> [Flag; 5] {
    [
        flag("path_prefix", Kind::Str, "/api", prefix_usage),
        flag(
            "trigger_max_retries",
            Kind::Int,
            "3",
            "Maximum retries for HTTP 429 errors from triggers",
        ),
        flag(
            "trigger_base_delay",
            Kind::Duration,
            "1s",
            "Base delay for trigger retry exponential backoff",
        ),
        flag(
            "trigger_max_delay",
            Kind::Duration,
            "10s",
            "Maximum delay for trigger retry exponential backoff",
        ),
        flag(
            "trigger_max_concurrent_runs",
            Kind::Int,
            "100",
            "Maximum concurrent trigger runs",
        ),
    ]
}

const PUBSUB_FLAGS: [Flag; 5] =
    trigger_flags("Path prefix for the PubSub trigger endpoint. Default is '/api'.");
const EVENTARC_FLAGS: [Flag; 5] =
    trigger_flags("Path prefix for the Eventarc trigger endpoint. Default is '/api'.");

const PUBSUB: FlagSet = FlagSet {
    name: "pubsub",
    flags: &PUBSUB_FLAGS,
};

const EVENTARC: FlagSet = FlagSet {
    name: "eventarc",
    flags: &EVENTARC_FLAGS,
};

/// ADK names the api flag set "web", which is what its usage line prints.
const API: FlagSet = FlagSet {
    name: "web",
    flags: &[
        flag(
            "webui_address",
            Kind::Str,
            "localhost:8080",
            "ADK WebUI origin as seen from the user browser. It is sent as the CORS allowed origin and is accepted by the REST server's origin check; every other cross-origin browser request is refused with 403. Accepts a full origin such as 'http://localhost:8080'; a bare hostname and optional port is read as http. '*' allows any origin and turns the origin check off - do not use it on a server reachable from an untrusted network, since these endpoints are unauthenticated.",
        ),
        flag(
            "path_prefix",
            Kind::Str,
            "/api",
            "ADK REST API path prefix. Default is '/api'.",
        ),
        flag(
            "sse-write-timeout",
            Kind::Duration,
            "2m0s",
            "SSE server write timeout (i.e. '10s', '2m' - see time.ParseDuration for details) - for writing the SSE response after reading the headers & body",
        ),
        flag(
            "trace_capacity",
            Kind::Int,
            "10000",
            "Maximum number of traces to keep in memory.",
        ),
        flag(
            "include_debug_api",
            Kind::Bool,
            "false",
            "Serve the debug trace and agent graph endpoints, which expose tool-call arguments, responses and tool names. The web UI's Traces and agent structure panels need them. !!! WARNING !!! : debug endpoints are not safe to be used in production environment, do not set them to true in production.",
        ),
    ],
};

/// The web sublaunchers in ADK's declaration order, which is also the order
/// their routes are mounted in.
const SUBLAUNCHERS: [(&str, &str, &FlagSet); 5] = [
    (
        "webui",
        "starts ADK Web UI server which provides UI for interacting with ADK REST API",
        &WEBUI,
    ),
    (
        "a2a",
        "starts A2A server which handles jsonrpc requests on /a2a/v1/invoke path",
        &A2A,
    ),
    (
        "pubsub",
        "starts ADK PubSub trigger endpoint server",
        &PUBSUB,
    ),
    (
        "eventarc",
        "starts ADK Eventarc trigger endpoint server",
        &EVENTARC,
    ),
    (
        "api",
        "starts ADK REST API server, accepting origins specified by webui_address (CORS)",
        &API,
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Streaming {
    None,
    Sse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleConfig {
    /// `None` picks SSE when stdout is a terminal, as ADK does.
    pub streaming: Option<Streaming>,
    pub otel_to_cloud: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebConfig {
    /// Go's `int`; an unusable port fails at listen time, as in Go.
    pub port: i64,
    /// The bind host; ADK treats an empty `-host` as the default.
    pub host: String,
    /// The four server timeouts in nanoseconds as given; see the accessors.
    pub write_timeout_ns: i64,
    pub read_timeout_ns: i64,
    pub idle_timeout_ns: i64,
    pub shutdown_timeout_ns: i64,
    pub otel_to_cloud: bool,
    pub h2c: bool,
    /// Bytes; zero or less selects ADK's 10 MiB.
    pub max_request_body_size: i64,
    pub webui: Option<WebUiConfig>,
    pub a2a: Option<A2aConfig>,
    pub pubsub: Option<TriggerConfig>,
    pub eventarc: Option<TriggerConfig>,
    pub api: Option<ApiConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebUiConfig {
    pub api_server_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A2aConfig {
    pub agent_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerConfig {
    pub path_prefix: String,
    pub max_retries: i64,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub max_concurrent_runs: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiConfig {
    pub webui_address: String,
    pub path_prefix: String,
    pub sse_write_timeout: Option<Duration>,
    pub include_debug_api: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Console(ConsoleConfig),
    Web(Box<WebConfig>),
}

/// The full launcher's `CommandLineSyntax()`, built from the flag tables.
pub fn syntax() -> String {
    let mut web = WEB.defaults();
    web.push_str("  You may specify sublaunchers:\n");
    for (kw, desc, _) in SUBLAUNCHERS {
        web.push_str(&format!("    * {kw} - {desc}\n"));
    }
    web.push_str("  Sublaunchers syntax:\n");
    for (kw, _, set) in SUBLAUNCHERS {
        web.push_str(&format!("    {kw}\n  {}\n", set.defaults()));
    }
    format!(
        "Arguments: Specify one of the following:\n  * console - runs an agent in console mode.\n  * web - starts web server with additional sub-servers specified by sublaunchers\nDetails:\n  console\n{}\n  web\n{web}\n\n",
        CONSOLE.defaults()
    )
}

/// Go's `strings.Fields`.
pub fn fields(s: &str) -> Vec<String> {
    s.split(char::is_whitespace)
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect()
}

/// The universal launcher's parse. Flag errors print the Go `flag` package's
/// message and usage to `stderr`; the returned error is what ADK returns.
pub fn parse(args: &[String], stderr: &mut dyn Write) -> Result<Launch, String> {
    let rest = match args.first().map(String::as_str) {
        Some("web") => return parse_web(&args[1..], stderr).map(|w| Launch::Web(Box::new(w))),
        Some("console") => &args[1..],
        _ => args,
    };
    let (values, remaining) = CONSOLE
        .parse(rest, stderr)
        .map_err(|e| format!("failed to parse flags: {e}"))?;
    let streaming = match values.str("streaming_mode").as_str() {
        "" => None,
        "none" => Some(Streaming::None),
        "sse" => Some(Streaming::Sse),
        other => {
            return Err(format!(
                "invalid streaming_mode: {other}. Should be (none|sse)"
            ));
        }
    };
    unparsed(&remaining)?;
    Ok(Launch::Console(ConsoleConfig {
        streaming,
        otel_to_cloud: values.bool("otel_to_cloud"),
    }))
}

fn unparsed(remaining: &[String]) -> Result<(), String> {
    if remaining.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "cannot parse following arguments: [{}]",
            remaining.join(" ")
        ))
    }
}

/// ADK's web launcher `Parse`: its own flags, then each sublauncher keyword
/// with its flags, until an unknown word.
fn parse_web(args: &[String], stderr: &mut dyn Write) -> Result<WebConfig, String> {
    let (web, mut rest) = WEB
        .parse(args, stderr)
        .map_err(|e| format!("failed to parse web flags: {e}"))?;
    let mut active: Vec<(&str, Values)> = Vec::new();
    while let Some(keyword) = rest.first().cloned() {
        if active.iter().any(|(k, _)| *k == keyword) {
            return Err(format!(
                "the keyword {} is specified and processed more than once, which is not allowed",
                yutu_auth::log::quote(&keyword)
            ));
        }
        let Some((kw, _, set)) = SUBLAUNCHERS.iter().find(|(k, _, _)| *k == keyword) else {
            break;
        };
        let cannot = |e: String| {
            format!(
                "the {} launcher cannot parse arguments: {e}",
                yutu_auth::log::quote(kw)
            )
        };
        let (values, remaining) = set
            .parse(&rest[1..], stderr)
            .map_err(|e| cannot(format!("failed to parse {kw} flags: {e}")))?;
        if matches!(*kw, "pubsub" | "eventarc") {
            check_trigger(&values).map_err(|e| cannot(e.into()))?;
        }
        active.push((kw, values));
        rest = remaining;
    }
    unparsed(&rest)?;
    let get = |k: &str| active.iter().find(|(kw, _)| *kw == k).map(|(_, v)| v);
    let trigger = |v: &Values| TriggerConfig {
        path_prefix: normalize_prefix(&v.str("path_prefix")),
        max_retries: v.int("trigger_max_retries"),
        base_delay: Duration::from_nanos(v.nanos("trigger_base_delay").max(0) as u64),
        max_delay: Duration::from_nanos(v.nanos("trigger_max_delay").max(0) as u64),
        max_concurrent_runs: v.int("trigger_max_concurrent_runs"),
    };
    Ok(WebConfig {
        port: web.int("port"),
        host: web.str("host"),
        write_timeout_ns: web.nanos("write-timeout"),
        read_timeout_ns: web.nanos("read-timeout"),
        idle_timeout_ns: web.nanos("idle-timeout"),
        shutdown_timeout_ns: web.nanos("shutdown-timeout"),
        otel_to_cloud: web.bool("otel_to_cloud"),
        h2c: web.bool("h2c"),
        max_request_body_size: web.int("max_request_body_size"),
        webui: get("webui").map(|v| WebUiConfig {
            api_server_address: v.str("api_server_address"),
        }),
        a2a: get("a2a").map(|v| A2aConfig {
            agent_url: v.str("a2a_agent_url"),
        }),
        pubsub: get("pubsub").map(trigger),
        eventarc: get("eventarc").map(trigger),
        api: get("api").map(|v| ApiConfig {
            webui_address: v.str("webui_address"),
            path_prefix: normalize_prefix(&v.str("path_prefix")),
            sse_write_timeout: v.timeout("sse-write-timeout"),
            include_debug_api: v.bool("include_debug_api"),
        }),
    })
}

fn check_trigger(v: &Values) -> Result<(), &'static str> {
    if v.int("trigger_max_retries") <= 0 {
        return Err("trigger_max_retries must be > 0");
    }
    if v.nanos("trigger_base_delay") < 0 {
        return Err("trigger_base_delay must be >= 0");
    }
    if v.nanos("trigger_max_delay") <= 0 {
        return Err("trigger_max_delay must be > 0");
    }
    if v.int("trigger_max_concurrent_runs") <= 0 {
        return Err("trigger_max_concurrent_runs must be > 0");
    }
    Ok(())
}

/// A leading slash added and a trailing one dropped, so "/" becomes "".
fn normalize_prefix(p: &str) -> String {
    let p = if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/{p}")
    };
    p.strip_suffix('/').unwrap_or(&p).to_string()
}

/// A Go server timeout: zero and negative mean none.
fn timeout(nanos: i64) -> Option<Duration> {
    u64::try_from(nanos)
        .ok()
        .filter(|n| *n > 0)
        .map(Duration::from_nanos)
}

impl WebConfig {
    pub fn write_timeout(&self) -> Option<Duration> {
        timeout(self.write_timeout_ns)
    }

    pub fn read_timeout(&self) -> Option<Duration> {
        timeout(self.read_timeout_ns)
    }

    pub fn idle_timeout(&self) -> Option<Duration> {
        timeout(self.idle_timeout_ns)
    }

    pub fn shutdown_timeout(&self) -> Option<Duration> {
        timeout(self.shutdown_timeout_ns)
    }

    /// The address the listener binds.
    pub fn bind_host(&self) -> &str {
        if self.host.is_empty() {
            "127.0.0.1"
        } else {
            &self.host
        }
    }

    /// ADK's 10 MiB unless `-max_request_body_size` is positive.
    pub fn body_cap(&self) -> u64 {
        u64::try_from(self.max_request_body_size)
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(10 << 20)
    }

    /// The URL ADK prints: loopback and wildcard hosts shown as localhost.
    pub fn web_url(&self) -> String {
        let host = match self.bind_host() {
            "127.0.0.1" | "::1" | "0.0.0.0" | "::" => "localhost",
            h => h,
        };
        if host.contains(':') {
            format!("http://[{host}]:{}", self.port)
        } else {
            format!("http://{host}:{}", self.port)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &str) -> (Result<Launch, String>, String) {
        let mut err = Vec::new();
        let r = parse(&fields(args), &mut err);
        (r, String::from_utf8(err).unwrap())
    }

    fn console(streaming: Option<Streaming>) -> Result<Launch, String> {
        Ok(Launch::Console(ConsoleConfig {
            streaming,
            otel_to_cloud: false,
        }))
    }

    fn web(args: &str) -> WebConfig {
        match run(args).0 {
            Ok(Launch::Web(w)) => *w,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn routes_like_the_universal_launcher() {
        assert_eq!(run("console").0, console(None));
        assert_eq!(run("").0, console(None));
        assert_eq!(
            run("  -streaming_mode=sse ").0,
            console(Some(Streaming::Sse))
        );
        assert_eq!(
            run("console --streaming_mode none").0,
            console(Some(Streaming::None))
        );
        assert_eq!(run("console -shutdown-timeout 1m30s").0, console(None));
        assert_eq!(
            run("foo bar").0,
            Err("cannot parse following arguments: [foo bar]".into())
        );
        assert_eq!(
            run("console -streaming_mode=foo").0,
            Err("invalid streaming_mode: foo. Should be (none|sse)".into())
        );
    }

    #[test]
    fn web_takes_its_flags_then_sublaunchers() {
        let w = web(
            "web -port 9000 -read-timeout 2s webui api -path_prefix x/ a2a -a2a_agent_url http://h",
        );
        assert_eq!(w.port, 9000);
        assert_eq!(w.read_timeout(), Some(Duration::from_secs(2)));
        assert_eq!(w.write_timeout(), Some(Duration::from_secs(15)));
        assert_eq!(w.idle_timeout(), Some(Duration::from_secs(60)));
        assert_eq!(w.bind_host(), "127.0.0.1");
        assert_eq!(w.body_cap(), 10 << 20);
        assert_eq!(
            w.webui.unwrap().api_server_address,
            "http://localhost:8080/api"
        );
        let api = w.api.unwrap();
        assert_eq!(api.path_prefix, "/x");
        assert_eq!(api.webui_address, "localhost:8080");
        assert_eq!(api.sse_write_timeout, Some(Duration::from_secs(120)));
        assert_eq!(w.a2a.unwrap().agent_url, "http://h");
        assert!(w.pubsub.is_none());
        let w = web("web pubsub -path_prefix / eventarc -trigger_max_retries 5");
        assert_eq!(w.pubsub.unwrap().path_prefix, "");
        assert_eq!(w.eventarc.unwrap().max_retries, 5);
        assert_eq!(web("web -host= api").bind_host(), "127.0.0.1");
    }

    #[test]
    fn web_errors_match_adk() {
        let err = |a: &str| run(a).0.unwrap_err();
        assert_eq!(err("web foo"), "cannot parse following arguments: [foo]");
        assert_eq!(
            err("web api api"),
            "the keyword \"api\" is specified and processed more than once, which is not allowed"
        );
        assert_eq!(
            err("web api -bogus"),
            "the \"api\" launcher cannot parse arguments: failed to parse api flags: flag provided but not defined: -bogus"
        );
        assert_eq!(
            err("web -port x"),
            "failed to parse web flags: invalid value \"x\" for flag -port: parse error"
        );
        assert_eq!(
            err("web pubsub -trigger_max_retries 0"),
            "the \"pubsub\" launcher cannot parse arguments: trigger_max_retries must be > 0"
        );
        let (_, stderr) = run("web webui -h");
        assert_eq!(stderr, format!("Usage of webui:\n{}", WEBUI.defaults()));
        let (_, stderr) = run("web api -x");
        assert!(stderr.starts_with(
            "flag provided but not defined: -x\nUsage of web:\n  -include_debug_api\n"
        ));
    }

    #[test]
    fn flag_errors_print_go_usage() {
        let usage = format!("Usage of console:\n{}", CONSOLE.defaults());
        let (r, err) = run("console -x");
        assert_eq!(
            r,
            Err("failed to parse flags: flag provided but not defined: -x".into())
        );
        assert_eq!(err, format!("flag provided but not defined: -x\n{usage}"));
        let (r, err) = run("console -h");
        assert_eq!(r, Err("failed to parse flags: flag: help requested".into()));
        assert_eq!(err, usage);
        let (r, _) = run("console -shutdown-timeout x");
        assert_eq!(
            r,
            Err(r#"failed to parse flags: invalid value "x" for flag -shutdown-timeout: parse error"#.into())
        );
        let (r, _) = run("-streaming_mode");
        assert_eq!(
            r,
            Err("failed to parse flags: flag needs an argument: -streaming_mode".into())
        );
        let (r, _) = run("console -otel_to_cloud=maybe");
        assert_eq!(
            r,
            Err(r#"failed to parse flags: invalid boolean value "maybe" for -otel_to_cloud: parse error"#.into())
        );
    }

    #[test]
    fn syntax_is_built_from_the_flag_tables() {
        assert_eq!(syntax(), SYNTAX);
    }
}
