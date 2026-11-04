//! Output for people in a terminal.

use std::io::IsTerminal;

use callme_core::env::{Source, Vars};
use callme_core::history::{BodyKind, Record};
use callme_core::project::RequestRef;
use callme_core::run::RunResult;
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

pub fn run(result: &RunResult, include: bool) {
    let r = &result.record;
    let env = r.env.as_deref().map(|e| format!("  ({e})")).unwrap_or_default();
    println!("{} {}{}", paint("1", &r.request.method), r.request.url, dim(&env));
    let resp = &r.response;
    println!(
        "{}  {}",
        paint(status_code(resp.status), &format!("{} {}", resp.status, resp.reason)),
        dim(&format!("{} · {} ms · {}", resp.http_version, r.timing.total, size(resp.size)))
    );
    if include {
        for (n, v) in &resp.headers {
            println!("{}: {}", paint("36", n), v);
        }
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
