//! Turning a parsed request into one ready to send: `{{…}}` replaced by environment variables, dynamic values
//! (`{{$uuid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, `{{$randomInt}}`) and response references
//! (`{{login.response.body.$.token}}`, `{{login.response.headers.Location}}`, taken from the referenced request's
//! newest history record), the body read, and `Content-Type: application/json` added to a JSON body without one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::env::Vars;
use crate::exec::Prepared;
use crate::history::{self, BodyKind};
use crate::parse::{Body, Request};
use crate::project::Project;
use crate::Error;

pub struct Context<'a> {
    pub project: &'a Project,
    /// The `.http` file the request is in, relative to the root.
    pub file: &'a str,
    pub vars: &'a Vars,
}

/// A prepared request and the headers callme added to it.
pub struct Resolved {
    pub prepared: Prepared,
    pub added_headers: Vec<String>,
}

fn var_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{\s*([^{}]+?)\s*\}\}").expect("valid regex"))
}

/// The expressions inside `{{…}}` in `text`, with their byte ranges.
pub fn references(text: &str) -> impl Iterator<Item = (std::ops::Range<usize>, &str)> {
    var_re().captures_iter(text).map(|c| (c.get(0).unwrap().range(), c.get(1).unwrap().as_str()))
}

impl Context<'_> {
    pub fn substitute(&self, text: &str) -> Result<String, Error> {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for (range, expr) in references(text) {
            out.push_str(&text[last..range.start]);
            out.push_str(&self.lookup(expr)?);
            last = range.end;
        }
        out.push_str(&text[last..]);
        Ok(out)
    }

    fn lookup(&self, expr: &str) -> Result<String, Error> {
        if let Some(name) = expr.strip_prefix('$') {
            return dynamic(name).ok_or_else(|| {
                Error::Usage(format!("不认识的动态变量 `{{{{{expr}}}}}`，支持 $uuid、$timestamp、$isoTimestamp、$randomInt"))
            });
        }
        if let Some((name, rest)) = expr.split_once(".response.") {
            return self.response_ref(name, rest);
        }
        self.vars.get(expr).map(str::to_string).ok_or_else(|| {
            let env = self.vars.env.as_deref().map(|e| format!("环境 {e}")).unwrap_or_else(|| "未选择环境".into());
            Error::Usage(format!("变量 `{expr}` 没有定义（{env}）"))
        })
    }

    fn response_ref(&self, name: &str, rest: &str) -> Result<String, Error> {
        let (file, request) = self.project.find_named(self.file, name)?;
        let record = history::latest(self.project, &file, &request.id()).ok_or_else(|| {
            Error::Usage(format!("请求 `{name}` 还没有运行过，先运行它：callme run {file}#{name}"))
        })?;
        if let Some(header) = rest.strip_prefix("headers.") {
            return record
                .response
                .header(header)
                .map(str::to_string)
                .ok_or_else(|| Error::Usage(format!("`{name}` 最近一次的响应里没有响应头 `{header}`")));
        }
        let path = match rest {
            "body" | "body.*" | "body.$" => None,
            r => match r.strip_prefix("body.") {
                Some(p) if p.starts_with('$') => Some(p.to_string()),
                Some(p) => Some(format!("$.{p}")),
                None => {
                    return Err(Error::Usage(format!(
                        "`{name}.response.{rest}` 写法不对，应该是 response.body.<JSONPath> 或 response.headers.<名字>"
                    )))
                }
            },
        };
        let Some(path) = path else {
            let body = history::full_body(self.project, &record).unwrap_or_default();
            return Ok(String::from_utf8_lossy(&body).into_owned());
        };
        if record.response.body_kind != BodyKind::Json {
            return Err(Error::Usage(format!("`{name}` 最近一次的响应不是 JSON，不能用 JSONPath 取值")));
        }
        let value = query(&record.response.body, &path)?
            .ok_or_else(|| Error::Usage(format!("`{name}` 最近一次的响应里，{path} 没有匹配的值")))?;
        Ok(match value {
            Value::String(s) => s,
            v => v.to_string(),
        })
    }

    pub fn prepare(&self, req: &Request, cookie_jar: Option<PathBuf>) -> Result<Resolved, Error> {
        let url = self.substitute(&req.url)?;
        let mut headers = Vec::with_capacity(req.headers.len());
        for h in &req.headers {
            headers.push((self.substitute(&h.name)?, self.substitute(&h.value)?));
        }
        let body = match &req.body {
            None => None,
            Some(Body::Text(t)) => Some(self.substitute(t)?.into_bytes()),
            Some(Body::File(p)) => {
                let base = Path::new(self.file).parent().unwrap_or(Path::new(""));
                let path = self.project.root.join(base).join(self.substitute(p)?);
                Some(std::fs::read(&path).map_err(|e| Error::Io(path.display().to_string(), e))?)
            }
        };
        let mut added_headers = Vec::new();
        let has_type = headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-type"));
        if let Some(b) = &body {
            if !has_type && serde_json::from_slice::<Value>(b).is_ok_and(|v| v.is_object() || v.is_array()) {
                headers.push(("Content-Type".into(), "application/json".into()));
                added_headers.push("Content-Type".into());
            }
        }
        Ok(Resolved {
            prepared: Prepared {
                method: req.method.clone(),
                url,
                http_version: req.http_version.clone(),
                headers,
                body,
                follow_redirects: !req.directives.no_redirect,
                insecure: req.directives.insecure,
                timeout: req.directives.timeout.map(std::time::Duration::from_secs_f64),
                cookie_jar: if req.directives.no_cookie_jar { None } else { cookie_jar },
            },
            added_headers,
        })
    }
}

/// The first value `path` (JSONPath, RFC 9535) selects in `value`.
pub fn query(value: &Value, path: &str) -> Result<Option<Value>, Error> {
    let p = serde_json_path::JsonPath::parse(path).map_err(|e| Error::Usage(format!("JSONPath `{path}` 不合法：{e}")))?;
    Ok(p.query(value).first().cloned())
}

/// Every value `path` selects in `value`, for filtering a response.
pub fn query_all(value: &Value, path: &str) -> Result<Vec<Value>, Error> {
    let p = serde_json_path::JsonPath::parse(path).map_err(|e| Error::Usage(format!("JSONPath `{path}` 不合法：{e}")))?;
    Ok(p.query(value).all().into_iter().cloned().collect())
}

fn dynamic(name: &str) -> Option<String> {
    let now = std::time::SystemTime::now();
    Some(match name {
        "uuid" | "random.uuid" => uuid::Uuid::new_v4().to_string(),
        "timestamp" => now.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs().to_string(),
        "isoTimestamp" => humantime::format_rfc3339_seconds(now).to_string(),
        "randomInt" => fastrand::u32(0..1000).to_string(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::EnvFiles;
    use crate::history::tests::record;
    use crate::parse::parse;

    fn setup() -> (tempfile::TempDir, Project, Vars) {
        let dir = tempfile::tempdir().unwrap();
        let p = Project { root: dir.path().to_path_buf() };
        std::fs::write(p.root.join("auth.http"), "# @name login\nPOST {{baseUrl}}/login\n").unwrap();
        let vars = EnvFiles::from_json(r#"{"dev": {"baseUrl": "https://dev.x"}}"#, r#"{"dev": {"token": "tok-123456"}}"#)
            .unwrap()
            .resolve(Some("dev"), &[])
            .unwrap();
        (dir, p, vars)
    }

    #[test]
    fn substitutes_variables_and_dynamic_values() {
        let (_d, p, vars) = setup();
        let cx = Context { project: &p, file: "auth.http", vars: &vars };
        assert_eq!(cx.substitute("{{baseUrl}}/a?t={{ token }}").unwrap(), "https://dev.x/a?t=tok-123456");
        assert_eq!(cx.substitute("{{$uuid}}").unwrap().len(), 36);
        let err = cx.substitute("{{nope}}").unwrap_err().to_string();
        assert!(err.contains("nope") && err.contains("dev"), "{err}");
    }

    #[test]
    fn response_references_read_history() {
        let (_d, p, vars) = setup();
        let cx = Context { project: &p, file: "auth.http", vars: &vars };
        let err = cx.substitute("{{login.response.body.$.token}}").unwrap_err().to_string();
        assert!(err.contains("还没有运行过"), "{err}");

        let mut r = record("auth.http", "login", "2026-11-04T06:32:00.000Z", serde_json::json!({"token": "abc", "user": {"id": 7}}));
        history::write(&p, &mut r, b"").unwrap();
        assert_eq!(cx.substitute("{{login.response.body.$.token}}").unwrap(), "abc");
        assert_eq!(cx.substitute("{{login.response.body.user.id}}").unwrap(), "7");
        assert_eq!(cx.substitute("{{login.response.headers.content-type}}").unwrap(), "application/json");
        assert!(cx.substitute("{{login.response.body.$.missing}}").is_err());
    }

    #[test]
    fn prepares_requests() {
        let (_d, p, vars) = setup();
        std::fs::write(p.root.join("body.json"), "{\"raw\": \"{{baseUrl}}\"}").unwrap();
        let cx = Context { project: &p, file: "auth.http", vars: &vars };
        let f = parse("# @no-cookie-jar\n# @timeout 3\nPOST {{baseUrl}}/a\nX-T: {{token}}\n\n{\"a\": 1}\n###\nPOST {{baseUrl}}/b\n\n< ./body.json\n");

        let a = cx.prepare(&f.requests[0], Some("jar".into())).unwrap();
        assert_eq!(a.prepared.url, "https://dev.x/a");
        assert_eq!(a.prepared.headers[0], ("X-T".into(), "tok-123456".into()));
        assert_eq!(a.prepared.headers[1], ("Content-Type".into(), "application/json".into()));
        assert_eq!(a.added_headers, vec!["Content-Type"]);
        assert_eq!(a.prepared.cookie_jar, None);
        assert_eq!(a.prepared.timeout, Some(std::time::Duration::from_secs(3)));

        let b = cx.prepare(&f.requests[1], Some("jar".into())).unwrap();
        assert_eq!(b.prepared.body.as_deref(), Some(&b"{\"raw\": \"{{baseUrl}}\"}"[..]), "file bodies are sent as is");
        assert_eq!(b.prepared.cookie_jar, Some(PathBuf::from("jar")));
    }
}
