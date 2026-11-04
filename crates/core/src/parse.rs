//! Parsing `.http` files: the subset of the JetBrains HTTP Client / VS Code REST Client syntax callme supports.
//!
//! A file is a list of requests separated by `###` lines. Each request has an optional preamble of comments and
//! directives (`# @name login`), a request line (`POST https://…`, the method defaulting to GET), query
//! continuation lines (`  ?a=1`, `  &b=2`), headers until the first blank line, then a body. Commented-out
//! headers are skipped, which is how a header is disabled. Scripts (`> {% … %}`) and file variables (`@x = 1`) are
//! not supported and reported as diagnostics. Lines are 0-based.

use serde::Serialize;

pub const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT"];

#[derive(Debug, Clone, Default, Serialize)]
pub struct HttpFile {
    pub requests: Vec<Request>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    /// 1-based position of the request in its file.
    pub index: usize,
    pub title: Option<String>,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub http_version: Option<String>,
    pub headers: Vec<Header>,
    pub body: Option<Body>,
    pub directives: Directives,
    /// The lines of the request's block, `start..end`.
    pub start_line: usize,
    pub end_line: usize,
    pub request_line: usize,
}

impl Request {
    /// How the request is addressed: its name, else `#<index>`.
    pub fn id(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("#{}", self.index),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Header {
    pub name: String,
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Body {
    Text(String),
    /// `< ./path`: the body is read from a file, relative to the `.http` file.
    File(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Directives {
    pub no_redirect: bool,
    pub no_cookie_jar: bool,
    pub insecure: bool,
    /// Seconds.
    pub timeout: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl HttpFile {
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error)
    }

    /// Finds a request by name, or by `#<index>` / `<index>`.
    pub fn find(&self, id: &str) -> Option<&Request> {
        let index = id.strip_prefix('#').unwrap_or(id).parse::<usize>().ok();
        self.requests
            .iter()
            .find(|r| r.name.as_deref() == Some(id))
            .or_else(|| index.and_then(|i| self.requests.iter().find(|r| r.index == i)))
    }
}

pub fn parse(text: &str) -> HttpFile {
    let lines: Vec<&str> = text.lines().collect();
    let mut file = HttpFile::default();

    // Blocks: (title, first line of content, end line).
    let mut blocks: Vec<(Option<String>, usize, usize)> = Vec::new();
    let mut start = 0;
    let mut title = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.trim_start().strip_prefix("###") {
            blocks.push((title.take(), start, i));
            let rest = rest.trim();
            title = (!rest.is_empty()).then(|| rest.to_string());
            start = i + 1;
        }
    }
    blocks.push((title, start, lines.len()));

    for (title, start, end) in blocks {
        if let Some(request) = parse_block(&lines, start, end, title, file.requests.len() + 1, &mut file.diagnostics) {
            file.requests.push(request);
        }
    }

    let mut seen: Vec<&str> = Vec::new();
    for r in &file.requests {
        if let Some(name) = r.name.as_deref() {
            if seen.contains(&name) {
                file.diagnostics.push(Diagnostic {
                    line: r.start_line,
                    severity: Severity::Error,
                    message: format!("请求名 `{name}` 重复"),
                });
            }
            seen.push(name);
        }
    }
    file
}

fn comment_text(line: &str) -> Option<&str> {
    let t = line.trim_start();
    t.strip_prefix('#').or_else(|| t.strip_prefix("//")).map(str::trim)
}

fn parse_block(
    lines: &[&str],
    start: usize,
    end: usize,
    title: Option<String>,
    index: usize,
    diags: &mut Vec<Diagnostic>,
) -> Option<Request> {
    let mut name = None;
    let mut directives = Directives::default();
    let mut i = start;

    // Preamble: blank lines, comments, directives.
    while i < end {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        if let Some(c) = comment_text(line) {
            if let Some(d) = c.strip_prefix('@') {
                parse_directive(d, i, &mut name, &mut directives, diags);
            }
            i += 1;
            continue;
        }
        if line.trim_start().starts_with('@') {
            diags.push(Diagnostic {
                line: i,
                severity: Severity::Error,
                message: "不支持文件变量（@name = value），请把变量写进 http-client.env.json".into(),
            });
            i += 1;
            continue;
        }
        break;
    }
    if i >= end {
        return None;
    }

    // Request line.
    let request_line = i;
    let mut tokens = lines[i].split_whitespace();
    let first = tokens.next().unwrap_or_default();
    let (method, mut url) = if METHODS.contains(&first) {
        match tokens.next() {
            Some(url) => (first.to_string(), url.to_string()),
            None => {
                diags.push(Diagnostic { line: i, severity: Severity::Error, message: format!("{first} 后面缺少 URL") });
                (first.to_string(), String::new())
            }
        }
    } else {
        ("GET".to_string(), first.to_string())
    };
    let http_version = tokens.next().filter(|t| t.starts_with("HTTP/")).map(str::to_string);
    i += 1;

    // Query continuation lines, then headers.
    while i < end {
        let t = lines[i].trim();
        if (t.starts_with('?') || t.starts_with('&')) && lines[i].starts_with(char::is_whitespace) {
            url.push_str(t);
            i += 1;
        } else {
            break;
        }
    }
    let mut headers = Vec::new();
    while i < end {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            break;
        }
        if comment_text(line).is_some() {
            i += 1;
            continue;
        }
        match line.split_once(':') {
            Some((n, v)) if !n.trim().is_empty() && !n.trim().contains(char::is_whitespace) => {
                headers.push(Header { name: n.trim().to_string(), value: v.trim().to_string(), line: i })
            }
            _ => diags.push(Diagnostic {
                line: i,
                severity: Severity::Error,
                message: "请求头应写成 `Name: value`；请求体前面要空一行".into(),
            }),
        }
        i += 1;
    }

    // Body.
    let mut body_lines: Vec<&str> = Vec::new();
    while i < end {
        let t = lines[i].trim_start();
        if t.starts_with("> ") || t.starts_with(">> ") || t.starts_with("<> ") || t.starts_with("> {%") {
            diags.push(Diagnostic {
                line: i,
                severity: Severity::Error,
                message: "不支持响应脚本和响应重定向（> {% … %}、>>、<>）".into(),
            });
            break;
        }
        body_lines.push(lines[i]);
        i += 1;
    }
    while body_lines.last().is_some_and(|l| l.trim().is_empty()) {
        body_lines.pop();
    }
    let body = if body_lines.is_empty() {
        None
    } else if body_lines.len() == 1 && body_lines[0].trim_start().starts_with("< ") {
        Some(Body::File(body_lines[0].trim_start()[2..].trim().to_string()))
    } else {
        Some(Body::Text(body_lines.join("\n")))
    };

    Some(Request {
        index,
        title,
        name,
        method,
        url,
        http_version,
        headers,
        body,
        directives,
        start_line: start,
        end_line: end,
        request_line,
    })
}

fn parse_directive(d: &str, line: usize, name: &mut Option<String>, dir: &mut Directives, diags: &mut Vec<Diagnostic>) {
    let (key, arg) = match d.split_once(char::is_whitespace) {
        Some((k, a)) => (k, a.trim()),
        None => (d.trim(), ""),
    };
    // REST Client also writes `@name=login`.
    let (key, arg) = match key.split_once('=') {
        Some((k, a)) => (k, a.trim()),
        None => (key, arg.trim_start_matches('=').trim()),
    };
    let mut err = |message: String| diags.push(Diagnostic { line, severity: Severity::Error, message });
    match key {
        "name" if arg.is_empty() => err("@name 后面缺少名字".into()),
        "name" if arg.contains(|c: char| c.is_whitespace() || c == '.' || c == '#') => {
            err("请求名不能包含空格、`.` 或 `#`".into())
        }
        "name" => *name = Some(arg.to_string()),
        "no-redirect" => dir.no_redirect = true,
        "no-cookie-jar" => dir.no_cookie_jar = true,
        "insecure" => dir.insecure = true,
        "timeout" => match arg.trim_end_matches('s').parse::<f64>() {
            Ok(s) if s > 0.0 => dir.timeout = Some(s),
            _ => err("@timeout 需要一个秒数，比如 `@timeout 5`".into()),
        },
        other => diags.push(Diagnostic { line, severity: Severity::Warning, message: format!("未知指令 @{other}，已忽略") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "### 登录
# @name login
POST {{baseUrl}}/auth/login
Content-Type: application/json
# X-Debug: 1

{\"user\": \"{{user}}\"}

### 获取当前用户
// @name me
// @no-redirect
// @timeout 2.5
GET {{baseUrl}}/me HTTP/1.1
Authorization: Bearer {{login.response.body.$.token}}
";

    #[test]
    fn parses_requests() {
        let f = parse(SAMPLE);
        assert!(f.diagnostics.is_empty(), "{:?}", f.diagnostics);
        assert_eq!(f.requests.len(), 2);

        let login = &f.requests[0];
        assert_eq!(login.title.as_deref(), Some("登录"));
        assert_eq!(login.name.as_deref(), Some("login"));
        assert_eq!(login.method, "POST");
        assert_eq!(login.url, "{{baseUrl}}/auth/login");
        assert_eq!(login.headers.len(), 1, "commented header is skipped");
        assert_eq!(login.body, Some(Body::Text("{\"user\": \"{{user}}\"}".into())));
        assert_eq!(login.request_line, 2);

        let me = &f.requests[1];
        assert_eq!(me.index, 2);
        assert_eq!(me.http_version.as_deref(), Some("HTTP/1.1"));
        assert!(me.directives.no_redirect);
        assert_eq!(me.directives.timeout, Some(2.5));
        assert_eq!(me.body, None);
        assert_eq!(f.find("me").unwrap().index, 2);
        assert_eq!(f.find("#1").unwrap().id(), "login");
    }

    #[test]
    fn bare_url_defaults_to_get_and_joins_query_lines() {
        let f = parse("https://example.com/search\n    ?q=a\n    &page=2\nAccept: */*\n");
        let r = &f.requests[0];
        assert_eq!(r.method, "GET");
        assert_eq!(r.url, "https://example.com/search?q=a&page=2");
        assert_eq!(r.id(), "#1");
        assert_eq!(r.headers[0].name, "Accept");
    }

    #[test]
    fn body_from_file() {
        let f = parse("POST https://x.dev\nContent-Type: application/json\n\n< ./body.json\n");
        assert_eq!(f.requests[0].body, Some(Body::File("./body.json".into())));
    }

    #[test]
    fn reports_unsupported_and_invalid_lines() {
        let f = parse("@host = x\nGET https://x.dev\nnot a header\n\n{}\n> {% client.global.set('a', 1) %}\n### \n# @name a\nGET /a\n### \n# @name a\nGET /b\n# @bogus\n");
        let msgs: Vec<_> = f.diagnostics.iter().map(|d| (d.line, d.severity)).collect();
        assert!(msgs.contains(&(0, Severity::Error)), "file variable");
        assert!(msgs.contains(&(2, Severity::Error)), "bad header");
        assert!(msgs.contains(&(5, Severity::Error)), "script");
        assert!(f.diagnostics.iter().any(|d| d.message.contains("重复")));
        assert_eq!(f.requests[0].body, Some(Body::Text("{}".into())));
    }

    #[test]
    fn comment_only_blocks_are_not_requests() {
        let f = parse("# just notes\n\n###\n\n");
        assert!(f.requests.is_empty());
    }
}
