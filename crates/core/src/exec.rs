//! Executing a resolved request with libcurl, which reports the time of each phase (DNS, connect, TLS, first byte,
//! download) and handles redirects, certificates, cookies, compression and HTTP/2.

use std::path::PathBuf;
use std::time::Duration;

use curl::easy::{Easy2, Handler, HttpVersion, List, WriteError};
use serde::{Deserialize, Serialize};

use crate::{Error, RequestErrorKind};

/// A request ready to send: variables substituted, body read.
#[derive(Debug, Clone, Default)]
pub struct Prepared {
    pub method: String,
    pub url: String,
    pub http_version: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub follow_redirects: bool,
    pub insecure: bool,
    pub timeout: Option<Duration>,
    /// The cookie jar to read and update; none: no cookies kept.
    pub cookie_jar: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timing: Timing,
}

/// Milliseconds spent in each phase; `total` is the whole request, redirects included.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub dns: f64,
    pub connect: f64,
    pub tls: f64,
    pub ttfb: f64,
    pub download: f64,
    pub total: f64,
}

/// Sees a request as it goes: what is sent, the head of each response (a redirect's too), then the final body in
/// chunks as they arrive. The CLI prints server-sent events with it; the GUI streams the body into its view.
pub trait Watcher {
    fn start(&mut self, _method: &str, _url: &str) {}
    fn head(&mut self, _status: u16, _headers: &[(String, String)]) {}
    fn chunk(&mut self, _data: &[u8]) {}
}

pub type OnChunk<'a> = Option<&'a mut dyn Watcher>;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// `curl/<libcurl's version> callme/<version>`: requests go out through libcurl, and gateways that let curl through
/// by its User-Agent (and turn unknown ones away) treat callme as they treat curl.
fn user_agent() -> String {
    format!("curl/{} callme/{}", curl::Version::get().version(), env!("CARGO_PKG_VERSION"))
}

struct Collector<'a> {
    headers: Vec<(String, String)>,
    status_line: String,
    body: Vec<u8>,
    on_chunk: OnChunk<'a>,
}

impl Handler for Collector<'_> {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.body.extend_from_slice(data);
        if let Some(w) = self.on_chunk.as_mut() {
            w.chunk(data);
        }
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        let line = String::from_utf8_lossy(data);
        let line = line.trim_end();
        if line.is_empty() {
            // The end of a head.
            if let Some(w) = self.on_chunk.as_mut() {
                let status = self.status_line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
                w.head(status, &self.headers);
            }
        } else if line.starts_with("HTTP/") {
            // A new response (after a redirect or a 100 Continue): only the last one counts.
            self.status_line = line.to_string();
            self.headers.clear();
        } else if let Some((n, v)) = line.split_once(':') {
            self.headers.push((n.trim().to_string(), v.trim().to_string()));
        }
        true
    }
}

/// Sends `req`; `on_chunk` sees the body as it arrives (server-sent events).
pub fn execute(req: &Prepared, on_chunk: OnChunk<'_>) -> Result<Response, Error> {
    let mut easy = Easy2::new(Collector { headers: vec![], status_line: String::new(), body: vec![], on_chunk });
    let fail = |e: curl::Error| request_error(&e);
    easy.url(&req.url).map_err(fail)?;
    easy.custom_request(&req.method).map_err(fail)?;
    if req.method == "HEAD" {
        easy.nobody(true).map_err(fail)?;
    }
    if let Some(body) = &req.body {
        easy.post_fields_copy(body).map_err(fail)?;
    }
    let mut list = List::new();
    let mut has_ua = false;
    for (n, v) in &req.headers {
        has_ua |= n.eq_ignore_ascii_case("user-agent");
        // `Name;` sends a header with an empty value; `Name:` would remove it.
        let line = if v.is_empty() { format!("{n};") } else { format!("{n}: {v}") };
        list.append(&line).map_err(fail)?;
    }
    let has = |name: &str| req.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    if req.body.is_some() && !has("expect") {
        list.append("Expect:").map_err(fail)?;
    }
    if req.body.is_some() && !has("content-type") {
        // libcurl would otherwise send its own `application/x-www-form-urlencoded`.
        list.append("Content-Type:").map_err(fail)?;
    }
    easy.http_headers(list).map_err(fail)?;
    if !has_ua {
        easy.useragent(&user_agent()).map_err(fail)?;
    }
    easy.accept_encoding("").map_err(fail)?;
    easy.follow_location(req.follow_redirects).map_err(fail)?;
    easy.max_redirections(20).map_err(fail)?;
    if req.insecure {
        easy.ssl_verify_peer(false).map_err(fail)?;
        easy.ssl_verify_host(false).map_err(fail)?;
    }
    easy.timeout(req.timeout.unwrap_or(DEFAULT_TIMEOUT)).map_err(fail)?;
    match req.http_version.as_deref() {
        Some("HTTP/1.0") => easy.http_version(HttpVersion::V10).map_err(fail)?,
        Some("HTTP/1.1") => easy.http_version(HttpVersion::V11).map_err(fail)?,
        Some("HTTP/2") | Some("HTTP/2.0") => easy.http_version(HttpVersion::V2).map_err(fail)?,
        _ => {}
    }
    if let Some(jar) = &req.cookie_jar {
        if let Some(dir) = jar.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        easy.cookie_file(jar).map_err(fail)?;
        easy.cookie_jar(jar).map_err(fail)?;
    }

    easy.perform().map_err(fail)?;

    let ms = |d: Result<Duration, curl::Error>| d.map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
    let dns = ms(easy.namelookup_time());
    let connect = ms(easy.connect_time());
    let tls_done = ms(easy.appconnect_time());
    let first_byte = ms(easy.starttransfer_time());
    let total = ms(easy.total_time());
    let ready = connect.max(tls_done);
    let round = |x: f64| (x.max(0.0) * 10.0).round() / 10.0;
    let timing = Timing {
        dns: round(dns),
        connect: round(connect - dns),
        tls: if tls_done > 0.0 { round(tls_done - connect) } else { 0.0 },
        ttfb: round(first_byte - ready),
        download: round(total - first_byte),
        total: round(total),
    };
    let status = easy.response_code().map_err(fail)? as u16;
    let collector = easy.get_mut();
    let mut parts = collector.status_line.splitn(3, ' ');
    let http_version = parts.next().unwrap_or("HTTP/1.1").to_string();
    let _code = parts.next();
    let reason = parts.next().map(str::to_string).unwrap_or_else(|| reason_phrase(status).to_string());
    Ok(Response {
        status,
        reason,
        http_version,
        headers: std::mem::take(&mut collector.headers),
        body: std::mem::take(&mut collector.body),
        timing,
    })
}

fn request_error(e: &curl::Error) -> Error {
    let (kind, hint) = if e.is_operation_timedout() {
        (RequestErrorKind::Timeout, Some("可以用 `# @timeout <秒>` 调大超时，默认 30 秒".to_string()))
    } else if e.is_peer_failed_verification() || e.is_ssl_cacert() || e.is_ssl_certproblem() {
        (RequestErrorKind::Tls, Some("证书校验失败；如果是自签名证书，可以在请求前加 `# @insecure`".to_string()))
    } else if e.is_ssl_connect_error() {
        (RequestErrorKind::Tls, Some("TLS 握手失败：确认地址和端口提供的是 HTTPS 服务，或检查代理设置".to_string()))
    } else if e.is_couldnt_resolve_host() {
        (RequestErrorKind::Network, Some("检查域名，或者当前环境里的 baseUrl".to_string()))
    } else if e.is_url_malformed() || e.is_unsupported_protocol() {
        (RequestErrorKind::Network, Some("URL 需要以 http:// 或 https:// 开头，变量是否已替换？".to_string()))
    } else {
        (RequestErrorKind::Network, None)
    };
    let extra = e.extra_description().map(|d| format!("（{}）", d.trim())).unwrap_or_default();
    Error::Request { kind, message: format!("{}{extra}", e.description()), hint }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}
