//! ADK's `internal/originguard` for the REST API: a cross-origin browser
//! request is served only from the web UI's origin, and on a loopback bind
//! a request naming a foreign Host is refused (DNS rebinding).

use std::net::IpAddr;

use super::http::Req;

/// `NormalizeOrigin`: a bare host reads as http, scheme and host lowercase.
pub fn normalize(addr: &str) -> String {
    let addr = addr.trim();
    if addr.is_empty() || addr == "*" {
        return addr.to_string();
    }
    let full = if addr.contains("://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    };
    let Some((scheme, rest)) = full.split_once("://") else {
        return addr.to_string();
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or_default();
    if host.is_empty() {
        return addr.to_string();
    }
    format!("{}://{}", scheme.to_lowercase(), host.to_lowercase())
}

fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return host;
        };
        let suffix = &rest[end + 1..];
        if suffix.is_empty()
            || suffix
                .strip_prefix(':')
                .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return &rest[..end];
        }
        return host;
    }
    if host.matches(':').count() == 1 {
        let (bare, port) = host.split_once(':').expect("one colon");
        if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
            return bare;
        }
    }
    host
}

fn bare_host(host: &str) -> String {
    strip_port(host)
        .to_lowercase()
        .trim_end_matches('.')
        .to_string()
}

fn is_loopback(host: &str) -> bool {
    let bare = bare_host(host);
    bare == "localhost" || bare.parse::<IpAddr>().is_ok_and(|a| a.is_loopback())
}

fn is_wildcard(host: &str) -> bool {
    let bare = bare_host(host);
    bare.is_empty() || bare.parse::<IpAddr>().is_ok_and(|a| a.is_unspecified())
}

fn first_value<'a>(req: &'a Req, name: &str) -> Option<&'a str> {
    let v = req.header(name)?;
    let first = v.split(',').next().unwrap_or_default().trim();
    (!first.is_empty()).then_some(first)
}

fn origin_host(origin: &str) -> String {
    let rest = origin.split_once("://").map_or(origin, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    bare_host(host)
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string()
}

/// The origin this request would have if it came from its own page.
fn request_origin(req: &Req) -> Option<String> {
    if let Some(fwd) = req.header("forwarded") {
        let first = fwd.split(',').next().unwrap_or_default();
        let (mut proto, mut host) = (None, None);
        for el in first.split(';') {
            if let Some((k, v)) = el.split_once('=') {
                let v = v.trim().trim_matches('"').to_string();
                match k.trim().to_lowercase().as_str() {
                    "proto" => proto = Some(v),
                    "host" => host = Some(v),
                    _ => {}
                }
            }
        }
        if let (Some(p), Some(h)) = (proto, host)
            && !p.is_empty()
            && !h.is_empty()
        {
            return Some(format!("{}://{}", scheme(&p), h.to_lowercase()));
        }
    }
    let host = first_value(req, "x-forwarded-host")
        .map(String::from)
        .or_else(|| req.header("host").map(String::from))?;
    let proto = first_value(req, "x-forwarded-proto").unwrap_or("http");
    Some(format!("{}://{}", scheme(proto), host.to_lowercase()))
}

fn scheme(s: &str) -> String {
    match s.to_lowercase().as_str() {
        "ws" => "http".into(),
        "wss" => "https".into(),
        other => other.into(),
    }
}

/// `Policy.Check` with ADK's api configuration: the web UI's address as
/// the one allowed origin, and the resolved bind host.
pub fn check(req: &Req, webui_address: &str, bind_host: &str) -> Result<(), &'static str> {
    let allowed = normalize(webui_address);
    if allowed == "*" {
        return Ok(());
    }
    let allowed_host = (!allowed.is_empty()).then(|| origin_host(&allowed));
    let loopback_bind = is_loopback(bind_host);
    if loopback_bind
        && let Some(host) = req.header("host").filter(|h| !h.is_empty())
        && (host.contains(',')
            || (!is_loopback(host) && allowed_host.as_deref() != Some(bare_host(host).as_str())))
    {
        return Err("host not allowed");
    }
    let Some(origin) = first_value(req, "origin") else {
        return Ok(());
    };
    if origin.len() > 2048 {
        return Err("origin not allowed");
    }
    let origin = normalize(origin);
    if origin == allowed {
        return Ok(());
    }
    let only_this_machine = if is_wildcard(bind_host) {
        req.local.ip().is_loopback()
    } else {
        loopback_bind
    };
    if only_this_machine && !is_loopback(&origin_host(&origin)) {
        return Err("origin not allowed");
    }
    if request_origin(req).as_deref() == Some(origin.as_str()) {
        Ok(())
    } else {
        Err("origin not allowed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_normalize_like_adk() {
        assert_eq!(normalize("localhost:8080"), "http://localhost:8080");
        assert_eq!(
            normalize("HTTP://LocalHost:8080/x"),
            "http://localhost:8080"
        );
        assert_eq!(normalize("*"), "*");
        assert_eq!(origin_host("http://[::1]:80"), "::1");
        assert!(is_loopback("127.0.0.1:8080"));
        assert!(is_loopback("[::1]:8080"));
        assert!(!is_loopback("evil.example:8080"));
    }
}
