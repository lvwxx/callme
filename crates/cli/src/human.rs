//! Output for people in a terminal.

use std::io::{IsTerminal, Write};

use callme_core::env::{Source, Vars};
use callme_core::history::{BodyKind, Record};
use callme_core::project::RequestRef;
use callme_core::exec::Watcher;
use callme_core::run::{RunResult, SseDecoder};
use serde_json::Value;
use callme_core::Error;

fn color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn paint(code: &str, text: &str) -> String {
    if color() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn dim(t: &str) -> String {
    paint("2", t)
}

fn status_code(status: u16) -> &'static str {
    match status {
        200..=299 => "1;32",
        300..=399 => "1;33",
        _ => "1;31",
    }
}

fn size(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.2} KB", n as f64 / 1024.0)
    } else {
        format!("{:.2} MB", n as f64 / 1024.0 / 1024.0)
    }
}

/// Prints a run as it goes: the request first, then server-sent events as they arrive.
#[derive(Default)]
pub struct Live {
    include: bool,
    sse: Option<SseDecoder>,
    events: usize,
}

impl Live {
    pub fn new(include: bool) -> Self {
        Live { include, ..Default::default() }
    }
}

impl Watcher for Live {
    fn start(&mut self, method: &str, url: &str) {
        println!("{} {}", paint("1", method), url);
        let _ = std::io::stdout().flush();
    }

    fn head(&mut self, status: u16, headers: &[(String, String)]) {
        let streaming = headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().starts_with("text/event-stream"));
        if streaming && !(300..400).contains(&status) {
            self.sse = Some(SseDecoder::default());
            println!("{}  {}", paint(status_code(status), &status.to_string()), dim("流式接收中…"));
            if self.include {
                print_headers(headers);
            }
            println!();
            let _ = std::io::stdout().flush();
        }
    }

    fn chunk(&mut self, data: &[u8]) {
        if let Some(d) = self.sse.as_mut() {
            let events = d.push(data);
            self.events += events.len();
            events.iter().for_each(print_event);
            let _ = std::io::stdout().flush();
        }
    }
}

fn print_event(e: &Value) {
    let name = e.get("event").and_then(Value::as_str).unwrap_or("message");
    let data = match e.get("data") {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    println!("{}  {}", paint("36", name), data);
}

fn print_headers(headers: &[(String, String)]) {
    for (n, v) in headers {
        println!("{}: {}", paint("36", n), v);
    }
}

/// The rest of a run after [`Live`] printed its request (and events).
pub fn finish(result: &RunResult, live: &mut Live) {
    let r = &result.record;
    let resp = &r.response;
    let env = r.env.as_deref().map(|e| format!(" · {e}")).unwrap_or_default();
    let summary = format!("{} · {} ms · {}{env}", resp.http_version, r.timing.total, size(resp.size));
    if let Some(d) = live.sse.as_mut() {
        let rest = d.finish();
        live.events += rest.len();
        rest.iter().for_each(print_event);
        println!("\n{}", dim(&format!("{} 个事件 · {summary}", live.events)));
        return;
    }
    println!("{}  {}", paint(status_code(resp.status), &format!("{} {}", resp.status, resp.reason)), dim(&summary));
    if live.include {
        print_headers(&resp.headers);
    }
    println!();
    body(r, Some(&result.raw_body));
}

fn body(r: &Record, raw: Option<&[u8]>) {
    let resp = &r.response;
    match resp.body_kind {
        BodyKind::Json | BodyKind::Sse => println!("{}", serde_json::to_string_pretty(&resp.body).unwrap()),
        BodyKind::Text => println!("{}", resp.body.as_str().unwrap_or_default()),
        BodyKind::Empty => println!("{}", dim("（空响应体）")),
        BodyKind::Binary => println!("{}", dim(&format!("（二进制内容，{}）", size(raw.map_or(resp.size, <[u8]>::len))))),
    }
    if let Some(f) = &resp.body_file {
        println!("{}", dim(&format!("响应体较大，已保存到 {f}")));
    }
}

pub fn list(items: &[RequestRef]) {
    if items.is_empty() {
        println!("{}", dim("没有找到 .http 文件"));
        return;
    }
    let width = items.iter().map(|r| r.file.len() + r.id.len() + 1).max().unwrap_or(0);
    for r in items {
        let target = format!("{}#{}", r.file, r.id);
        println!("{target:width$}  {:7} {}", paint("1", &r.method), r.url);
    }
}

pub fn env(names: &[String], current: Option<&str>, vars: &Vars) {
    if names.is_empty() {
        println!("{}", dim("没有环境：在项目根目录创建 http-client.env.json"));
    } else {
        let list: Vec<String> = names
            .iter()
            .map(|n| if Some(n.as_str()) == current { paint("1;36", &format!("{n}（当前）")) } else { n.clone() })
            .collect();
        println!("环境：{}", list.join("  "));
    }
    let shown = vars.env.as_deref().unwrap_or("$shared");
    println!("\n{} 的变量：", paint("1", shown));
    let width = vars.vars.keys().map(String::len).max().unwrap_or(0);
    for (k, v) in &vars.vars {
        let value = if v.source == Source::Private { "***".to_string() } else { v.value.clone() };
        let source = match v.source {
            Source::Shared => "$shared",
            Source::Env => "环境",
            Source::Private => "私有",
            Source::Override => "覆盖",
        };
        println!("  {k:width$}  {value}  {}", dim(source));
    }
}

pub fn history(records: &[(String, Record)]) {
    if records.is_empty() {
        println!("{}", dim("还没有运行记录"));
        return;
    }
    for (path, r) in records {
        let resp = &r.response;
        println!(
            "{}  {}  {:>8}  {:8}  {}",
            r.at,
            paint(status_code(resp.status), &resp.status.to_string()),
            format!("{} ms", r.timing.total),
            r.env.as_deref().unwrap_or("-"),
            dim(path)
        );
    }
}

pub fn error(e: &Error) {
    let red = |t: &str| if std::io::stderr().is_terminal() { format!("\x1b[1;31m{t}\x1b[0m") } else { t.to_string() };
    eprintln!("{} {e}", red("错误："));
    if let Some(h) = e.hint() {
        eprintln!("提示：{h}");
    }
}
