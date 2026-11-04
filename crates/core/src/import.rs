//! Importing a request from a curl command (DevTools "Copy as cURL", docs) or a `fetch(…)` call (DevTools "Copy as
//! fetch") into `.http` text. With the project's variables, the URL's base becomes `{{baseUrl}}` (any variable whose
//! value starts it) and secrets found in headers and body become their variables. Browser noise headers
//! (`sec-ch-*`, `sec-fetch-*`, `priority`, …) are dropped.

use serde_json::Value;

use crate::env::{Source, Vars};
use crate::Error;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub insecure: bool,
}

/// The `.http` text and what was replaced by variables, for the user to see.
#[derive(Debug, Clone, PartialEq)]
pub struct Converted {
    pub text: String,
    pub replaced: Vec<String>,
}

pub fn parse_any(text: &str) -> Result<Imported, Error> {
    let t = text.trim();
    if t.starts_with("curl ") || t.starts_with("curl\t") || t == "curl" {
        parse_curl(t)
    } else if t.contains("fetch(") {
        parse_fetch(t)
    } else {
        Err(Error::Usage("没有识别出 curl 命令或 fetch(…) 调用".into()))
    }
}

pub fn parse_curl(cmd: &str) -> Result<Imported, Error> {
    let joined = cmd.replace("\\\r\n", " ").replace("\\\n", " ");
    let words = shell_words::split(&ansi_c_quotes(&joined)).map_err(|e| Error::Usage(format!("curl 命令的引号不匹配：{e}")))?;
    let mut it = words.into_iter().peekable();
    if it.peek().map(String::as_str) == Some("curl") {
        it.next();
    }
    let mut imp = Imported::default();
    let mut method: Option<String> = None;
    let mut data: Vec<String> = Vec::new();
    let mut get = false;
    let mut head = false;

    const WITH_VALUE: &[char] = &['X', 'H', 'd', 'u', 'b', 'A', 'e', 'o', 'm', 'w', 'x', 'F', 'T', 'c'];
    while let Some(word) = it.next() {
        // Split `-XPOST`, `-sSL` and `--opt=value`.
        let (opt, attached): (String, Option<String>) = if let Some(long) = word.strip_prefix("--") {
            match long.split_once('=') {
                Some((o, v)) => (format!("--{o}"), Some(v.to_string())),
                None => (word.clone(), None),
            }
        } else if word.starts_with('-') && word.len() > 2 {
            let first = word.chars().nth(1).unwrap();
            if WITH_VALUE.contains(&first) {
                (format!("-{first}"), Some(word[2..].to_string()))
            } else {
                for c in word[1..].chars() {
                    match c {
                        'k' => imp.insecure = true,
                        'I' => head = true,
                        'G' => get = true,
                        _ => {}
                    }
                }
                continue;
            }
        } else {
            (word.clone(), None)
        };
        let mut value = |name: &str| -> Result<String, Error> {
            attached.clone().or_else(|| it.next()).ok_or_else(|| Error::Usage(format!("curl 参数 {name} 缺少值")))
        };
        match opt.as_str() {
            "-X" | "--request" => method = Some(value(&opt)?.to_uppercase()),
            "-H" | "--header" => {
                let h = value(&opt)?;
                match h.split_once(':') {
                    Some((n, v)) => imp.headers.push((n.trim().to_string(), v.trim().to_string())),
                    None => return Err(Error::Usage(format!("请求头 `{h}` 格式不对"))),
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" | "--data-urlencode" => data.push(value(&opt)?),
            "--json" => {
                data.push(value(&opt)?);
                imp.headers.push(("Content-Type".into(), "application/json".into()));
                imp.headers.push(("Accept".into(), "application/json".into()));
            }
            "-u" | "--user" => imp.headers.push(("Authorization".into(), format!("Basic {}", base64(value(&opt)?.as_bytes())))),
            "-b" | "--cookie" => imp.headers.push(("Cookie".into(), value(&opt)?)),
            "-A" | "--user-agent" => imp.headers.push(("User-Agent".into(), value(&opt)?)),
            "-e" | "--referer" => imp.headers.push(("Referer".into(), value(&opt)?)),
            "--url" => imp.url = value(&opt)?,
            "-k" | "--insecure" => imp.insecure = true,
            "-I" | "--head" => head = true,
            "-G" | "--get" => get = true,
            "-F" | "--form" => return Err(Error::Usage("暂不支持 multipart 表单（-F），请手动改写请求体".into())),
            "-o" | "--output" | "-m" | "--max-time" | "--connect-timeout" | "-w" | "--write-out" | "-x" | "--proxy"
            | "--retry" | "-c" | "--cookie-jar" | "-T" | "--upload-file" => {
                value(&opt)?;
            }
            o if o.starts_with('-') => {}
            _ => imp.url = word,
        }
    }
    if imp.url.is_empty() {
        return Err(Error::Usage("curl 命令里没有 URL".into()));
    }
    if get && !data.is_empty() {
        let sep = if imp.url.contains('?') { '&' } else { '?' };
        imp.url = format!("{}{sep}{}", imp.url, data.join("&"));
        data.clear();
    }
    imp.body = (!data.is_empty()).then(|| data.join("&"));
    imp.method = method.unwrap_or_else(|| {
        if head {
            "HEAD".into()
        } else if imp.body.is_some() {
            "POST".into()
        } else {
            "GET".into()
        }
    });
    Ok(imp)
}

pub fn parse_fetch(code: &str) -> Result<Imported, Error> {
    let start = code.find("fetch(").ok_or_else(|| Error::Usage("没有找到 fetch(".into()))? + "fetch(".len();
    let rest = code[start..].trim_start();
    let (url, after) = js_string(rest).ok_or_else(|| Error::Usage("fetch 的第一个参数应该是字符串 URL".into()))?;
    let mut imp = Imported { method: "GET".into(), url, ..Default::default() };
    let after = after.trim_start();
    if let Some(opts) = after.strip_prefix(',') {
        let opts = opts.trim_start();
        let end = opts.rfind('}').ok_or_else(|| Error::Usage("fetch 的第二个参数不完整".into()))?;
        let value: Value = serde_json::from_str(&opts[..=end])
            .map_err(|e| Error::Usage(format!("无法解析 fetch 的第二个参数（需要是 JSON 形式）：{e}")))?;
        if let Some(m) = value.get("method").and_then(Value::as_str) {
            imp.method = m.to_uppercase();
        }
        if let Some(Value::Object(h)) = value.get("headers") {
            for (n, v) in h {
                imp.headers.push((n.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())));
            }
        }
        if let Some(r) = value.get("referrer").and_then(Value::as_str) {
            if !r.is_empty() && !imp.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("referer")) {
                imp.headers.push(("Referer".into(), r.to_string()));
            }
        }
        match value.get("body") {
            Some(Value::String(s)) => imp.body = Some(s.clone()),
            Some(Value::Null) | None => {}
            Some(v) => imp.body = Some(v.to_string()),
        }
    }
    Ok(imp)
}

const NOISE: &[&str] = &["priority", "upgrade-insecure-requests", "dnt", "content-length", "host", "connection"];

/// Writes `imp` as a `.http` request.
pub fn to_http(imp: &Imported, vars: Option<&Vars>, name: Option<&str>) -> Converted {
    let mut replaced = Vec::new();
    let mut url = imp.url.clone();
    if let Some(vars) = vars {
        // The longest variable value that starts the URL (a base URL).
        let base = vars
            .vars
            .iter()
            .filter(|(_, v)| v.value.starts_with("http") && v.value.len() > "https://".len())
            .filter(|(_, v)| {
                let b = v.value.trim_end_matches('/');
                url.starts_with(b) && matches!(url[b.len()..].chars().next(), None | Some('/' | '?' | '#'))
            })
            .max_by_key(|(_, v)| v.value.len());
        if let Some((k, v)) = base {
            url = format!("{{{{{k}}}}}{}", &url[v.value.trim_end_matches('/').len()..]);
            replaced.push(k.clone());
        }
    }
    let secrets: Vec<(&String, &str)> = vars
        .map(|v| {
            let mut s: Vec<(&String, &str)> = v
                .vars
                .iter()
                .filter(|(_, x)| x.source == Source::Private && x.value.len() >= 6)
                .map(|(k, x)| (k, x.value.as_str()))
                .collect();
            s.sort_by_key(|(_, x)| std::cmp::Reverse(x.len()));
            s
        })
        .unwrap_or_default();
    let mut secret = |text: &str| -> String {
        let mut out = text.to_string();
        for (k, v) in &secrets {
            if out.contains(v) {
                out = out.replace(v, &format!("{{{{{k}}}}}"));
                if !replaced.contains(k) {
                    replaced.push((*k).clone());
                }
            }
        }
        out
    };

    let mut text = String::new();
    if let Some(n) = name {
        text.push_str(&format!("# @name {n}\n"));
    }
    if imp.insecure {
        text.push_str("# @insecure\n");
    }
    // The query on continuation lines when it has several parameters.
    match url.split_once('?') {
        Some((path, query)) if query.contains('&') => {
            text.push_str(&format!("{} {}\n", imp.method, path));
            for (i, part) in query.split('&').enumerate() {
                text.push_str(&format!("    {}{}\n", if i == 0 { '?' } else { '&' }, part));
            }
        }
        _ => text.push_str(&format!("{} {}\n", imp.method, url)),
    }
    for (n, v) in &imp.headers {
        let lower = n.to_ascii_lowercase();
        if lower.starts_with("sec-ch-") || lower.starts_with("sec-fetch-") || NOISE.contains(&lower.as_str()) {
            continue;
        }
        text.push_str(&format!("{n}: {}\n", secret(v)));
    }
    if let Some(body) = &imp.body {
        let pretty = serde_json::from_str::<Value>(body)
            .ok()
            .filter(|v| v.is_object() || v.is_array())
            .map(|v| serde_json::to_string_pretty(&v).unwrap())
            .unwrap_or_else(|| body.clone());
        text.push('\n');
        text.push_str(&secret(&pretty));
        text.push('\n');
    }
    Converted { text, replaced }
}

/// Rewrites bash's `$'…'` strings (DevTools uses them for bodies with special characters) as double-quoted ones,
/// which `shell_words` understands.
fn ansi_c_quotes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let (mut single, mut double) = (false, false);
    while let Some(c) = chars.next() {
        if !single && !double && c == '$' && chars.peek() == Some(&'\'') {
            chars.next();
            let mut decoded = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\'' => break,
                    '\\' => match chars.next() {
                        Some('n') => decoded.push('\n'),
                        Some('t') => decoded.push('\t'),
                        Some('r') => decoded.push('\r'),
                        Some('x') => {
                            let hex: String = chars.by_ref().take(2).collect();
                            if let Ok(b) = u8::from_str_radix(&hex, 16) {
                                decoded.push(b as char);
                            }
                        }
                        Some('u') => {
                            let hex: String = chars.by_ref().take(4).collect();
                            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                decoded.push(ch);
                            }
                        }
                        Some(other) => decoded.push(other),
                        None => {}
                    },
                    c => decoded.push(c),
                }
            }
            out.push('"');
            for c in decoded.chars() {
                if matches!(c, '"' | '\\' | '$' | '`') {
                    out.push('\\');
                }
                out.push(c);
            }
            out.push('"');
            continue;
        }
        match c {
            '\'' if !double => single = !single,
            '"' if !single => double = !double,
            '\\' if !single => {
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// A JS string literal at the start of `s` and what follows it.
fn js_string(s: &str) -> Option<(String, &str)> {
    let quote = s.chars().next().filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in s.char_indices().skip(1) {
        if escaped {
            out.push(match c {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((out, &s[i + 1..]));
        } else {
            out.push(c);
        }
    }
    None
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::EnvFiles;

    #[test]
    fn parses_devtools_curl() {
        let cmd = r#"curl 'https://dev.api.example.com/v1/orders?status=open&limit=20' \
  -H 'accept: application/json' \
  -H 'authorization: Bearer eyJhbGciOiJIUzI1NiJ9.abc' \
  -H 'sec-ch-ua: "Chromium"' \
  -H 'sec-fetch-mode: cors' \
  --compressed"#;
        let imp = parse_curl(cmd).unwrap();
        assert_eq!(imp.method, "GET");
        assert_eq!(imp.url, "https://dev.api.example.com/v1/orders?status=open&limit=20");
        assert_eq!(imp.headers.len(), 4);

        let vars = EnvFiles::from_json(
            r#"{"dev": {"baseUrl": "https://dev.api.example.com"}}"#,
            r#"{"dev": {"token": "eyJhbGciOiJIUzI1NiJ9.abc"}}"#,
        )
        .unwrap()
        .resolve(Some("dev"), &[])
        .unwrap();
        let c = to_http(&imp, Some(&vars), Some("orders"));
        assert_eq!(
            c.text,
            "# @name orders\nGET {{baseUrl}}/v1/orders\n    ?status=open\n    &limit=20\naccept: application/json\nauthorization: Bearer {{token}}\n"
        );
        assert_eq!(c.replaced, vec!["baseUrl", "token"]);
    }

    #[test]
    fn parses_curl_options() {
        let imp = parse_curl("curl -sSL -XPUT https://x.dev/a -u ada:pw -k --data-raw $'{\"note\":\"it\\'s\\n\"}' -H 'Content-Type: application/json'").unwrap();
        assert_eq!(imp.method, "PUT");
        assert!(imp.insecure);
        assert_eq!(imp.headers[0], ("Authorization".into(), "Basic YWRhOnB3".into()));
        assert_eq!(imp.body.as_deref(), Some("{\"note\":\"it's\n\"}"));

        let imp = parse_curl("curl https://x.dev/s -G -d q=a -d page=2").unwrap();
        assert_eq!((imp.method.as_str(), imp.url.as_str()), ("GET", "https://x.dev/s?q=a&page=2"));

        let imp = parse_curl("curl https://x.dev -d 'a=1'").unwrap();
        assert_eq!(imp.method, "POST");
        assert!(parse_curl("curl -F a=@f https://x.dev").is_err());
    }

    #[test]
    fn parses_devtools_fetch() {
        let code = r#"fetch("https://x.dev/api/items", {
  "headers": { "accept": "*/*", "content-type": "application/json" },
  "referrer": "https://x.dev/",
  "body": "{\"name\":\"a\"}",
  "method": "POST",
  "mode": "cors"
});"#;
        let imp = parse_any(code).unwrap();
        assert_eq!(imp.method, "POST");
        assert_eq!(imp.headers.len(), 3);
        let c = to_http(&imp, None, None);
        assert!(c.text.ends_with("\n{\n  \"name\": \"a\"\n}\n"), "{}", c.text);
        assert_eq!(parse_any("fetch('https://x.dev')").unwrap().url, "https://x.dev");
    }

    #[test]
    fn base64_matches() {
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
    }
}
