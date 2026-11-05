//! What an editor asks about a `.http` text it holds (not yet saved): diagnostics, completions, hovers, the
//! resolved URL of a request, the request as a curl command, and an environment's variables. Positions are 0-based
//! lines and UTF-16 columns, as editors and language servers count them.

use serde::Serialize;

use crate::env::{EnvFiles, Source, Vars, PRIVATE_FILE, PUBLIC_FILE};
use crate::lang::{self, CompletionContext};
use crate::parse::{self, Severity};
use crate::project::Project;
use crate::resolve::Context;
use crate::run::current_env;
use crate::Error;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
}

impl CompletionItem {
    fn new(label: impl Into<String>, kind: &'static str) -> Self {
        CompletionItem { label: label.into(), kind, detail: None, documentation: None, insert_text: None }
    }
    fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
    fn insert(mut self, t: impl Into<String>) -> Self {
        self.insert_text = Some(t.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvValue {
    pub env: String,
    /// `••••••` for a secret.
    pub value: Option<String>,
    pub source: Option<Source>,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hover {
    pub name: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    /// `variable`, `response` (a response reference) or `dynamic`.
    pub kind: &'static str,
    pub values: Vec<EnvValue>,
    /// For a response reference: its value in the newest response, or why there is none.
    pub value: Option<String>,
    pub note: Option<String>,
    /// Where the variable is defined: `http-client.env.json:4`.
    pub defined_in: Option<String>,
}

/// A definition: `file` (relative to the root) and its 0-based `line`; `start..end` is the `{{…}}` it is for, in
/// UTF-16 columns of the line asked about.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub file: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VarRow {
    pub name: String,
    pub value: String,
    pub source: Source,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvView {
    pub envs: Vec<String>,
    pub current: Option<String>,
    pub env: Option<String>,
    pub vars: Vec<VarRow>,
}

const MASK: &str = "••••••";

pub struct Service<'a> {
    pub project: &'a Project,
    /// The file the text is, relative to the root.
    pub file: &'a str,
    pub text: &'a str,
    /// The environment; none: the current one.
    pub env: Option<String>,
}

impl Service<'_> {
    fn env_name(&self) -> Option<String> {
        self.env.clone().or_else(|| current_env(self.project))
    }

    fn vars(&self) -> Result<(EnvFiles, Vars), Error> {
        let files = self.project.env_files()?;
        let env = self.env_name().filter(|e| files.names().contains(e));
        let vars = files.resolve(env.as_deref(), &[])?;
        Ok((files, vars))
    }

    fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let lines = self.lines();
        let width = |l: usize| lines.get(l).map_or(0, |s| s.encode_utf16().count());
        let parsed = parse::parse(self.text);
        let mut out: Vec<Diagnostic> = parsed
            .diagnostics
            .iter()
            .map(|d| Diagnostic { line: d.line, start: 0, end: width(d.line), severity: d.severity, message: d.message.clone() })
            .collect();
        let vars = self.vars().map(|(_, v)| v).ok();
        let names: Vec<String> = self.project.list().map(|l| l.into_iter().filter_map(|r| r.name).collect()).unwrap_or_default();
        let local: Vec<String> = parsed.requests.iter().filter_map(|r| r.name.clone()).collect();
        for (line, range, expr) in lang::all_variables(self.text) {
            let src = lines[line];
            let (start, end) = (lang::utf16_col(src, range.start), lang::utf16_col(src, range.end));
            let mut push = |severity, message: String| out.push(Diagnostic { line, start, end, severity, message });
            if let Some(name) = expr.strip_prefix('$') {
                if !lang::DYNAMIC.iter().any(|(d, _)| &d[1..] == name) {
                    push(Severity::Error, format!("不认识的动态变量 `{expr}`"));
                }
            } else if let Some((name, _)) = expr.split_once(".response.") {
                if !local.iter().any(|n| n == name) && !names.iter().any(|n| n == name) {
                    push(Severity::Error, format!("没有名为 `{name}` 的请求"));
                }
            } else if let Some(v) = &vars {
                if v.get(&expr).is_none() {
                    let env = v.env.as_deref().map(|e| format!("环境 {e}")).unwrap_or_else(|| "未选择环境".into());
                    push(Severity::Warning, format!("变量 `{expr}` 没有定义（{env}）"));
                }
            }
        }
        out
    }

    pub fn completions(&self, line: usize, col16: usize) -> Vec<CompletionItem> {
        let lines = self.lines();
        let Some(src) = lines.get(line) else { return vec![] };
        let col = lang::byte_col(src, col16);
        match lang::completion_context(self.text, line, col) {
            CompletionContext::Variable { prefix } => self.variable_items(&prefix),
            CompletionContext::HeaderName => {
                let mut items: Vec<CompletionItem> =
                    lang::HEADERS.iter().map(|(h, _)| CompletionItem::new(*h, "property").detail("请求头").insert(format!("{h}: "))).collect();
                // Headers this project uses that are not common ones.
                for h in self.used_headers() {
                    if !items.iter().any(|i| i.label.eq_ignore_ascii_case(&h)) {
                        items.push(CompletionItem::new(h.clone(), "property").detail("本项目用过").insert(format!("{h}: ")));
                    }
                }
                items
            }
            CompletionContext::HeaderValue { name } => lang::HEADERS
                .iter()
                .find(|(h, _)| h.eq_ignore_ascii_case(&name))
                .map(|(_, values)| values.iter().map(|v| CompletionItem::new(*v, "value")).collect())
                .unwrap_or_default(),
            CompletionContext::Method => parse::METHODS.iter().map(|m| CompletionItem::new(*m, "keyword").insert(format!("{m} "))).collect(),
            CompletionContext::Directive => {
                lang::DIRECTIVES.iter().map(|(d, doc)| CompletionItem::new(format!("@{d}"), "keyword").detail(*doc)).collect()
            }
            CompletionContext::None => vec![],
        }
    }

    fn variable_items(&self, prefix: &str) -> Vec<CompletionItem> {
        // `login.` → the parts of a response reference.
        if let Some((name, rest)) = prefix.split_once('.') {
            let _ = rest;
            return vec![
                CompletionItem::new(format!("{name}.response.body.$."), "reference").detail("响应体里的值（JSONPath）"),
                CompletionItem::new(format!("{name}.response.headers."), "reference").detail("响应头"),
            ];
        }
        let mut items = Vec::new();
        if let Ok((_, vars)) = self.vars() {
            for (k, v) in &vars.vars {
                let shown = if v.source == Source::Private { MASK.to_string() } else { v.value.clone() };
                items.push(CompletionItem::new(k.clone(), "variable").detail(shown).insert(format!("{k}}}}}")));
            }
        }
        let mut names: Vec<String> = parse::parse(self.text).requests.into_iter().filter_map(|r| r.name).collect();
        if let Ok(list) = self.project.list() {
            names.extend(list.into_iter().filter_map(|r| r.name));
        }
        names.sort();
        names.dedup();
        for n in names {
            items.push(CompletionItem::new(n.clone(), "reference").detail("请求的响应").insert(format!("{n}.response.body.$.")));
        }
        for (d, doc) in lang::DYNAMIC {
            items.push(CompletionItem::new(*d, "constant").detail(*doc).insert(format!("{d}}}}}")));
        }
        items
    }

    fn used_headers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |text: &str| {
            for r in parse::parse(text).requests {
                for h in r.headers {
                    if !out.iter().any(|x| x.eq_ignore_ascii_case(&h.name)) {
                        out.push(h.name);
                    }
                }
            }
        };
        add(self.text);
        for f in self.project.http_files() {
            if let Ok(t) = std::fs::read_to_string(self.project.root.join(&f)) {
                add(&t);
            }
        }
        out
    }

    pub fn hover(&self, line: usize, col16: usize) -> Option<Hover> {
        let lines = self.lines();
        let src = lines.get(line)?;
        let (expr, range) = lang::variable_at(src, lang::byte_col(src, col16))?;
        let (start, end) = (lang::utf16_col(src, range.start), lang::utf16_col(src, range.end));
        let base = Hover { name: expr.clone(), line, start, end, kind: "variable", values: vec![], value: None, note: None, defined_in: None };
        if let Some(d) = expr.strip_prefix('$') {
            let doc = lang::DYNAMIC.iter().find(|(n, _)| &n[1..] == d).map(|(_, doc)| doc.to_string());
            return Some(Hover { kind: "dynamic", note: doc.or(Some("不认识的动态变量".into())), ..base });
        }
        if expr.contains(".response.") {
            let (_, vars) = self.vars().ok()?;
            let cx = Context { project: self.project, file: self.file, vars: &vars };
            let (value, note) = match cx.substitute(&format!("{{{{{expr}}}}}")) {
                Ok(v) => (Some(v), Some("取自最近一次响应".to_string())),
                Err(e) => (None, Some(e.to_string())),
            };
            return Some(Hover { kind: "response", value, note, ..base });
        }
        let files = self.project.env_files().ok()?;
        let current = self.env_name();
        let values = files
            .names()
            .into_iter()
            .map(|env| {
                let vars = files.resolve(Some(&env), &[]).ok();
                let var = vars.as_ref().and_then(|v| v.vars.get(&expr));
                EnvValue {
                    current: current.as_deref() == Some(env.as_str()),
                    value: var.map(|v| if v.source == Source::Private { MASK.into() } else { v.value.clone() }),
                    source: var.map(|v| v.source),
                    env,
                }
            })
            .collect();
        let defined_in = definition(self.project, current.as_deref(), &expr);
        Some(Hover { values, defined_in, ..base })
    }

    /// Where the `{{…}}` at `line`, UTF-16 `col16` is defined: an environment variable's line in its environment
    /// file, a response reference's request line. Lines are 0-based.
    pub fn definition(&self, line: usize, col16: usize) -> Option<Definition> {
        let lines = self.lines();
        let src = lines.get(line)?;
        let (expr, range) = lang::variable_at(src, lang::byte_col(src, col16))?;
        let link = (lang::utf16_col(src, range.start), lang::utf16_col(src, range.end));
        if expr.starts_with('$') {
            return None;
        }
        if let Some((name, _)) = expr.split_once(".response.") {
            // The current text first: it may hold the request, unsaved.
            let parsed = parse::parse(self.text);
            if let Some(r) = parsed.requests.iter().find(|r| r.name.as_deref() == Some(name)) {
                return Some(Definition { file: self.file.to_string(), line: r.request_line, start: link.0, end: link.1 });
            }
            let (file, r) = self.project.find_named(self.file, name).ok()?;
            return Some(Definition { file, line: r.request_line, start: link.0, end: link.1 });
        }
        let at = definition(self.project, self.env_name().as_deref(), &expr)?;
        let (file, n) = at.rsplit_once(':')?;
        Some(Definition { file: file.to_string(), line: n.parse::<usize>().ok()?.saturating_sub(1), start: link.0, end: link.1 })
    }

    /// The method and URL request `index` would be sent with (secrets masked).
    pub fn preview(&self, index: usize) -> Result<(String, String), Error> {
        let parsed = parse::parse(self.text);
        let r = parsed.requests.iter().find(|r| r.index == index).ok_or_else(|| Error::Usage("没有这个请求".into()))?;
        let (_, vars) = self.vars()?;
        let cx = Context { project: self.project, file: self.file, vars: &vars };
        Ok((r.method.clone(), vars.mask(&cx.substitute(&r.url)?)))
    }

    /// Request `index` as a curl command, secrets included (it is for running).
    pub fn curl(&self, index: usize) -> Result<String, Error> {
        let parsed = parse::parse(self.text);
        let r = parsed.requests.iter().find(|r| r.index == index).ok_or_else(|| Error::Usage("没有这个请求".into()))?;
        let (_, vars) = self.vars()?;
        let cx = Context { project: self.project, file: self.file, vars: &vars };
        let p = cx.prepare(r, None)?.prepared;
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let mut out = String::from("curl");
        // curl's own method: GET without a body, POST with one.
        let implied = if p.body.is_some() { "POST" } else { "GET" };
        if p.method != implied {
            out.push_str(&format!(" -X {}", p.method));
        }
        out.push_str(&format!(" {}", q(&p.url)));
        for (n, v) in &p.headers {
            out.push_str(&format!(" \\\n  -H {}", q(&format!("{n}: {v}"))));
        }
        if let Some(b) = &p.body {
            out.push_str(&format!(" \\\n  --data-raw {}", q(&String::from_utf8_lossy(b))));
        }
        if p.insecure {
            out.push_str(" -k");
        }
        if p.follow_redirects {
            out.push_str(" -L");
        }
        Ok(out)
    }
}

/// Diagnostics of an environment file's text (`file` is `http-client.env.json` or the private one): JSON that does
/// not parse, and — in the shared file — an environment missing a variable that another environment has and a
/// request of the project uses.
pub fn env_file_diagnostics(project: &Project, file: &str, text: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = text.split('\n').collect();
    let width = |l: usize| lines.get(l).map_or(0, |s| s.encode_utf16().count());
    if text.trim().is_empty() {
        return vec![];
    }
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let line = e.line().saturating_sub(1);
            return vec![Diagnostic { line, start: 0, end: width(line), severity: Severity::Error, message: format!("不是合法的 JSON：{e}") }];
        }
    };
    let Some(envs) = value.as_object() else {
        return vec![Diagnostic { line: 0, start: 0, end: width(0), severity: Severity::Error, message: "顶层应该是对象，每个键是一个环境".into() }];
    };
    if file != PUBLIC_FILE {
        return vec![];
    }
    // The variables the project's requests use.
    let mut used: Vec<String> = Vec::new();
    for f in project.http_files() {
        if let Ok(t) = std::fs::read_to_string(project.root.join(&f)) {
            for (_, _, expr) in lang::all_variables(&t) {
                if !expr.starts_with('$') && !expr.contains(".response.") && !used.contains(&expr) {
                    used.push(expr);
                }
            }
        }
    }
    let private = project.env_files().ok();
    let shared: Vec<&String> = envs.get("$shared").and_then(|v| v.as_object()).map(|m| m.keys().collect()).unwrap_or_default();
    let mut out = Vec::new();
    for (name, vars) in envs.iter().filter(|(n, _)| *n != "$shared") {
        let Some(vars) = vars.as_object() else { continue };
        let in_private = |var: &str| {
            private.as_ref().and_then(|p| p.resolve(Some(name), &[]).ok()).is_some_and(|v| v.is_secret(var))
        };
        let mut missing: Vec<&str> = Vec::new();
        for (other, other_vars) in envs.iter().filter(|(n, _)| *n != "$shared" && *n != name) {
            let _ = other;
            for k in other_vars.as_object().into_iter().flat_map(|m| m.keys()) {
                if !vars.contains_key(k) && !shared.contains(&k) && used.contains(k) && !in_private(k) && !missing.contains(&k.as_str()) {
                    missing.push(k);
                }
            }
        }
        if missing.is_empty() {
            continue;
        }
        let key = format!("\"{name}\"");
        let Some(line) = lines.iter().position(|l| l.contains(&key) && l[l.find(&key).unwrap() + key.len()..].trim_start().starts_with(':')) else { continue };
        let col = lines[line].find(&key).unwrap();
        out.push(Diagnostic {
            line,
            start: lang::utf16_col(lines[line], col),
            end: lang::utf16_col(lines[line], col + key.len()),
            severity: Severity::Warning,
            message: format!("{name} 没有 {}：其他环境有，请求里也用到了", missing.join("、")),
        });
    }
    out
}

/// An environment's variables, secrets masked.
pub fn env_view(project: &Project, env: Option<&str>) -> Result<EnvView, Error> {
    let files = project.env_files()?;
    let current = current_env(project);
    let shown = env.map(str::to_string).or_else(|| current.clone()).filter(|e| files.names().contains(e));
    let vars = files.resolve(shown.as_deref(), &[])?;
    let rows = vars
        .vars
        .iter()
        .map(|(k, v)| VarRow { name: k.clone(), value: if v.source == Source::Private { MASK.into() } else { v.value.clone() }, source: v.source })
        .collect();
    Ok(EnvView { envs: files.names(), current, env: shown, vars: rows })
}

/// Where `name` is defined for `env`: the private file first, then the public one, the environment's block before
/// `$shared`; `file:line` (1-based). Found by text: the first `"name":` from the line that opens the block.
fn definition(project: &Project, env: Option<&str>, name: &str) -> Option<String> {
    let key = format!("\"{name}\"");
    let is_key = |line: &str, k: &str| line.match_indices(k).any(|(i, _)| line[i + k.len()..].trim_start().starts_with(':'));
    let blocks: Vec<&str> = env.into_iter().chain(["$shared"]).collect();
    for block in blocks {
        for file in [PRIVATE_FILE, PUBLIC_FILE] {
            let Ok(text) = std::fs::read_to_string(project.root.join(file)) else { continue };
            let lines: Vec<&str> = text.lines().collect();
            let Some(open) = lines.iter().position(|l| is_key(l, &format!("\"{block}\""))) else { continue };
            if let Some(i) = (open..lines.len()).find(|&i| is_key(lines[i], &key)) {
                return Some(format!("{file}:{}", i + 1));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::write(
            root.join(PUBLIC_FILE),
            "{\n  \"$shared\": { \"v\": \"1\" },\n  \"dev\": {\n    \"baseUrl\": \"https://dev.x\"\n  },\n  \"prod\": { \"baseUrl\": \"https://x\" }\n}",
        )
        .unwrap();
        std::fs::write(root.join(PRIVATE_FILE), "{\"dev\": {\"token\": \"secret-123\"}}").unwrap();
        std::fs::write(root.join("auth.http"), "# @name login\nPOST {{baseUrl}}/login\nX-Trace: 1\n").unwrap();
        (dir, Project { root })
    }

    const TEXT: &str = "### 我\n# @name me\nGET {{baseUrl}}/me?t={{token}}\nAuthorization: Bearer {{login.response.body.$.token}}\nX: {{nope}} {{$bad}} {{ghost.response.body.$.a}}\n\n{\"a\": 1}";

    fn svc<'a>(p: &'a Project, env: &str) -> Service<'a> {
        Service { project: p, file: "me.http", text: TEXT, env: Some(env.into()) }
    }

    #[test]
    fn diagnoses_variables() {
        let (_d, p) = project();
        let d = svc(&p, "dev").diagnostics();
        let msgs: Vec<&str> = d.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(d.len(), 3, "{msgs:?}");
        assert!(msgs[0].contains("nope") && d[0].severity == Severity::Warning);
        assert_eq!((d[0].line, d[0].start, d[0].end), (4, 3, 11));
        assert!(msgs[1].contains("$bad"));
        assert!(msgs[2].contains("ghost"));
        // prod has no token.
        assert!(svc(&p, "prod").diagnostics().iter().any(|d| d.message.contains("token")));
    }

    #[test]
    fn completes_by_context() {
        let (_d, p) = project();
        let t = "GET {{ba\nAcc\nX-T\nAccept: ";
        let s = Service { project: &p, file: "x.http", text: t, env: Some("dev".into()) };
        let vars = s.completions(0, 8);
        let base = vars.iter().find(|i| i.label == "baseUrl").unwrap();
        assert_eq!(base.insert_text.as_deref(), Some("baseUrl}}"));
        assert_eq!(vars.iter().find(|i| i.label == "token").unwrap().detail.as_deref(), Some(MASK));
        assert!(vars.iter().any(|i| i.label == "login" && i.kind == "reference"));
        assert!(vars.iter().any(|i| i.label == "$uuid"));
        assert!(s.completions(1, 3).iter().any(|i| i.label == "Accept"));
        assert!(s.completions(2, 3).iter().any(|i| i.label == "X-Trace" && i.detail.as_deref() == Some("本项目用过")));
        assert!(s.completions(3, 8).iter().any(|i| i.label == "application/json"));
    }

    #[test]
    fn hovers_variables() {
        let (_d, p) = project();
        let h = svc(&p, "dev").hover(2, 8).unwrap();
        assert_eq!(h.name, "baseUrl");
        assert_eq!(h.values.len(), 2);
        assert!(h.values[0].current && h.values[0].value.as_deref() == Some("https://dev.x"));
        assert_eq!(h.defined_in.as_deref(), Some("http-client.env.json:4"));
        let t = svc(&p, "dev").hover(2, 24).unwrap();
        assert_eq!(t.values[0].value.as_deref(), Some(MASK));
        assert_eq!(t.defined_in.as_deref(), Some("http-client.private.env.json:1"));
        let r = svc(&p, "dev").hover(3, 30).unwrap();
        assert_eq!(r.kind, "response");
        assert!(r.note.unwrap().contains("还没有运行过"));
    }

    #[test]
    fn finds_definitions() {
        let (_d, p) = project();
        let s = svc(&p, "dev");
        assert_eq!(s.definition(2, 8), Some(Definition { file: PUBLIC_FILE.into(), line: 3, start: 4, end: 15 }));
        assert_eq!(s.definition(2, 24).unwrap().file, PRIVATE_FILE);
        assert_eq!(s.definition(3, 30), Some(Definition { file: "auth.http".into(), line: 1, start: 22, end: 53 }));
        assert_eq!(s.definition(2, 1), None);
        let t = "# @name a\nGET /a\n###\nGET {{a.response.body.$.x}}";
        let local = Service { project: &p, file: "x.http", text: t, env: None };
        assert_eq!(local.definition(3, 8).unwrap(), Definition { file: "x.http".into(), line: 1, start: 4, end: 27 });
    }

    #[test]
    fn previews_and_exports() {
        let (_d, p) = project();
        let s = svc(&p, "dev");
        assert_eq!(s.preview(1).unwrap(), ("GET".into(), "https://dev.x/me?t=***".into()));
        let t = "POST {{baseUrl}}/a\nX-T: {{token}}\n\n{\"it's\": 1}";
        let s = Service { project: &p, file: "x.http", text: t, env: Some("dev".into()) };
        assert_eq!(
            s.curl(1).unwrap(),
            "curl 'https://dev.x/a' \\\n  -H 'X-T: secret-123' \\\n  -H 'Content-Type: application/json' \\\n  --data-raw '{\"it'\\''s\": 1}' -L"
        );
    }

    #[test]
    fn diagnoses_environment_files() {
        let (_d, p) = project();
        std::fs::write(p.root.join("me.http"), TEXT).unwrap();
        let text = std::fs::read_to_string(p.root.join(PUBLIC_FILE)).unwrap();
        let d = env_file_diagnostics(&p, PUBLIC_FILE, &text);
        // prod lacks nothing the requests use but `token`, which dev has only privately.
        assert!(d.is_empty(), "{d:?}");
        let text = "{\n  \"dev\": { \"baseUrl\": \"a\", \"nope\": \"x\" },\n  \"prod\": { \"baseUrl\": \"b\" }\n}";
        let d = env_file_diagnostics(&p, PUBLIC_FILE, text);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].line, d[0].start, d[0].end), (2, 2, 8));
        assert!(d[0].message.contains("prod 没有 nope"));
        let bad = env_file_diagnostics(&p, PRIVATE_FILE, "{\n  \"dev\": {,}\n}");
        assert_eq!((bad[0].line, bad[0].severity), (1, Severity::Error));
    }

    #[test]
    fn views_environments() {
        let (_d, p) = project();
        let v = env_view(&p, Some("dev")).unwrap();
        assert_eq!(v.envs, vec!["dev", "prod"]);
        let token = v.vars.iter().find(|r| r.name == "token").unwrap();
        assert_eq!((token.value.as_str(), token.source), (MASK, Source::Private));
    }
}
