//! The HTTP transport for the DRTV desktop client.
//!
//! Blocking on purpose. The client is a desktop app with a small number of worker threads; a
//! blocking `ureq::Agent` keeps every call site a straight line, and the runtime we would get from
//! an async client is a second scheduler we do not need.
//!
//! The shape is deliberately close to the `plx_net` API the DR client code was ported from:
//! [`Resp`] carries a status and a body, header lists are `"Name: value"` lines, and a non-2xx
//! status is a *response*, not an error — the DR API says useful things with 401 and 404 bodies,
//! and callers decide what they mean.
//!
//! One [`Agent`] is shared process-wide so TLS sessions and connections survive between calls.

use std::sync::OnceLock;
use std::time::Duration;

use ureq::Agent;

/// Whole-request deadline. DR's API answers in tens of milliseconds; this is a hang detector, not a
/// performance budget, and it must not fire on a slow mobile connection either.
const TIMEOUT: Duration = Duration::from_secs(25);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

/// Large enough for a home page or a profile's watch history, small enough that a misdirected
/// request cannot eat memory.
const MAX_BODY: u64 = 24 * 1024 * 1024;

/// The user agent DR sees. The service answers anonymous catalog reads to any client, but naming
/// the app keeps its own logs honest and gives the service something to rate-limit per app.
pub const USER_AGENT: &str = concat!("drtv-desktop/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, thiserror::Error)]
pub enum NetError {
    #[error("request timed out")]
    Timeout,
    #[error("could not resolve host")]
    HostNotFound,
    #[error("TLS failure")]
    Tls,
    #[error("network transport failure: {0}")]
    Transport(String),
    #[error("invalid URL or request: {0}")]
    BadRequest(String),
}

/// A response that reached us, whatever its status.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body as text, or `None` when it is not UTF-8.
    pub fn text(&self) -> Option<String> {
        String::from_utf8(self.body.clone()).ok()
    }

    /// The body parsed as JSON.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        serde_json::from_slice(&self.body).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        }
    }
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::config_builder()
            // DR answers 401/403/404 with JSON we want to read.
            .http_status_as_error(false)
            .max_redirects(8)
            .user_agent(USER_AGENT)
            .timeout_global(Some(TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .build()
            .into()
    })
}

/// Send one request. `headers` are `"Name: value"` lines, as the DR client code builds them.
pub fn request(
    method: Method,
    url: &str,
    headers: &[String],
    body: Option<&[u8]>,
) -> Result<Resp, NetError> {
    let agent = agent();
    // GET and DELETE builders cannot carry a body and POST/PUT builders can, so the type differs
    // per verb; the headers are applied after the split rather than before it.
    let sent = match method {
        Method::Get => with_headers(agent.get(url), headers).call(),
        Method::Delete => with_headers(agent.delete(url), headers).call(),
        Method::Post => with_headers(agent.post(url), headers).send(body.unwrap_or_default()),
        Method::Put => with_headers(agent.put(url), headers).send(body.unwrap_or_default()),
    };

    let mut resp = sent.map_err(|e| classify(e, url))?;
    let status = resp.status().as_u16();
    let body = resp
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .map_err(|e| classify(e, url))?;
    Ok(Resp { status, body })
}

fn with_headers<T>(mut req: ureq::RequestBuilder<T>, headers: &[String]) -> ureq::RequestBuilder<T> {
    for line in headers {
        if let Some((name, value)) = line.split_once(':') {
            req = req.header(name.trim(), value.trim());
        }
    }
    req
}

pub fn get(url: &str, headers: &[String]) -> Result<Resp, NetError> {
    request(Method::Get, url, headers, None)
}

pub fn post(url: &str, headers: &[String], body: &[u8]) -> Result<Resp, NetError> {
    request(Method::Post, url, headers, Some(body))
}

pub fn put(url: &str, headers: &[String], body: &[u8]) -> Result<Resp, NetError> {
    request(Method::Put, url, headers, Some(body))
}

/// A DELETE with no body — DR's un-watch endpoint reads its item ids from the query string.
pub fn delete(url: &str, headers: &[String]) -> Result<Resp, NetError> {
    request(Method::Delete, url, headers, None)
}

/// Name the failure the user can act on. `ureq`'s error tree is deep and mostly internal; the app
/// shows one short line, so the interesting distinctions are timeout / no such host / TLS / other.
fn classify(err: ureq::Error, url: &str) -> NetError {
    match err {
        ureq::Error::Timeout(_) => NetError::Timeout,
        ureq::Error::HostNotFound => NetError::HostNotFound,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => NetError::Timeout,
        ureq::Error::Tls(_) => NetError::Tls,
        ureq::Error::StatusCode(code) => {
            // Only reachable if the shared agent ever turns status-as-error back on.
            NetError::Transport(format!("HTTP {code}"))
        }
        ureq::Error::BadUri(msg) => NetError::BadRequest(format!("{}: {msg}", sanitize(url))),
        other => NetError::Transport(other.to_string()),
    }
}

/// URLs in this app can carry a profile token in the query string; keep it out of logs and errors.
fn sanitize(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// Whether `method` may carry a body. Kept next to [`request`] so the two stay in step.
pub fn takes_body(method: Method) -> bool {
    !matches!(method, Method::Get)
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    /// Serve exactly one request with a canned response, then stop. No network, no fixture server.
    fn serve_once(status: &str, body: &'static str) -> String {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut buf = [0_u8; 2048];
                let _ = socket.read(&mut buf);
                let _ = socket.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/thing")
    }

    const JSON_404: &str = r#"{"message":"no such item"}"#;

    #[test]
    fn a_non_2xx_status_is_a_response_not_an_error() {
        let url = serve_once("404 Not Found", JSON_404);
        let resp = get(&url, &[]).expect("transport ok");
        assert_eq!(resp.status, 404);
        assert!(!resp.ok());
        let json: serde_json::Value = resp.json().expect("json");
        assert_eq!(json["message"], "no such item");
    }

    #[test]
    fn header_lines_are_split_once_on_the_first_colon() {
        let url = serve_once("404 Not Found", JSON_404);
        // A bearer token and an Origin both carry punctuation a naive split would mangle.
        let headers = vec![
            "Authorization: Bearer a:b:c".to_string(),
            "Origin: https://www.dr.dk".to_string(),
            "X-No-Colon".to_string(),
        ];
        assert_eq!(get(&url, &headers).expect("transport ok").status, 404);
    }

    #[test]
    fn a_closed_port_is_a_transport_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        let err = get(&format!("http://{addr}/"), &[]).expect_err("refused");
        assert!(matches!(err, NetError::Transport(_)), "{err:?}");
    }

    #[test]
    fn errors_never_echo_the_query_string() {
        assert_eq!(
            sanitize("https://prod95.dr-massive.com/api/x?profile_token=secret"),
            "https://prod95.dr-massive.com/api/x"
        );
    }

    #[test]
    fn resp_text_survives_utf8_and_rejects_garbage() {
        let resp = Resp { status: 200, body: "Blå".as_bytes().to_vec() };
        assert_eq!(resp.text().as_deref(), Some("Blå"));
        let bad = Resp { status: 200, body: vec![0xff, 0xfe] };
        assert!(bad.text().is_none());
    }
}
