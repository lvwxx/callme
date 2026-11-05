//! callme's native side: what `App.tsx` imports from `callme/native`. The `.http` language and its highlighter for
//! the editor, and `Callme`, the project operations (list, read, write, run) on callme-core. Requests run on a
//! blocking thread with libcurl; what arrives while they run (each response head, server-sent events) is posted to
//! the TSX callback as JSON.

use std::path::PathBuf;
use std::time::Duration;

use callme_core::exec::Watcher;
use callme_core::lang::{self, Token};
use callme_core::output;
use callme_core::history;
use callme_core::import;
use callme_core::project::Project;
use callme_core::service::{self, Service};
use callme_core::run::{self, RunOptions, SseDecoder};
use nsh_base::editor::{
    register_highlighter, register_language, EditorHighlighter, EditorLanguage, EditorText, EditorTextEdit, EditorToken,
    EditorTokens,
};
use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use serde_json::{json, Value};

/// The largest body the GUI receives inline; a longer one is cut (the whole body stays in history).
const MAX_BODY: usize = 8 * 1024 * 1024;

static HTTP: EditorLanguage = EditorLanguage::new("HTTP")
    .aliases(&["http", "rest"])
    .extensions(&["http", "rest"])
    .line_comment("#")
    .brackets(&[('{', '}'), ('[', ']')])
    .auto_close(&[('{', '}'), ('[', ']'), ('"', '"')])
    .word_chars("-$")
    .tab_size(2);

/// The crate's UI set (`nsh.json` `"rust"`).
#[native_module("callme/native")]
fn native(registry: &mut Registry) {
    registry.setup(|| {
        register_language(&HTTP);
        register_highlighter("http", || Box::new(HttpHighlighter::default()));
    });
    registry.module::<Callme>();
}

/// Highlights `.http` text with callme-core's tokens. Files are small, so each pass tokenizes the whole text and
/// gives the lines from the first edited one on.
#[derive(Default)]
struct HttpHighlighter {
    frontier: usize,
}

impl EditorHighlighter for HttpHighlighter {
    fn edited(&mut self, edit: &EditorTextEdit) {
        self.frontier = self.frontier.min(edit.first_line);
    }

    fn highlight(&mut self, text: &EditorText, last_line: usize, _budget: Duration, tokens: &mut EditorTokens) -> bool {
        let source = text.rope().to_string();
        let lines = lang::tokens(&source);
        let end = last_line.min(lines.len().saturating_sub(1));
        for (n, line) in lines.iter().enumerate().take(end + 1).skip(self.frontier) {
            tokens.line(n, line.iter().map(|(r, t)| (r.clone(), editor_token(*t))));
        }
        self.frontier = end + 1;
        true
    }

    fn frontier(&self) -> usize {
        self.frontier
    }
}

fn editor_token(t: Token) -> EditorToken {
    match t {
        Token::Comment => EditorToken::Comment,
        Token::Title => EditorToken::Title,
        Token::Keyword => EditorToken::Keyword,
        Token::Attr => EditorToken::Attr,
        Token::Variable | Token::Version => EditorToken::Built,
        Token::String => EditorToken::String,
        Token::Number => EditorToken::Number,
    }
}

/// Forwards what arrives during a run to the engine thread as JSON messages.
struct Forward {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
    sse: Option<SseDecoder>,
}

impl Watcher for Forward {
    fn head(&mut self, status: u16, headers: &[(String, String)]) {
        let streaming = headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().starts_with("text/event-stream"));
        if streaming && !(300..400).contains(&status) {
            self.sse = Some(SseDecoder::default());
        }
        let msg = json!({ "type": "head", "status": status, "headers": output::headers_object(headers), "streaming": streaming });
        let _ = self.tx.send(msg.to_string());
    }

    fn chunk(&mut self, data: &[u8]) {
        if let Some(d) = self.sse.as_mut() {
            for event in d.push(data) {
                let _ = self.tx.send(json!({ "type": "event", "event": event }).to_string());
            }
        }
    }
}

fn last_project_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".callme").join("last-project"))
}

fn recent_projects() -> Vec<String> {
    last_project_file()
        .and_then(|f| std::fs::read_to_string(f.with_file_name("recent-projects.json")).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn last_project() -> Option<PathBuf> {
    let text = std::fs::read_to_string(last_project_file()?).ok()?;
    let path = text.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// A project-relative `.http` path from what was typed: trimmed, `.http` added, no `..`.
fn normalize_http(name: &str) -> Result<String, String> {
    let name = name.trim().trim_start_matches('/');
    if name.is_empty() || name.split('/').any(|p| p == ".." || p.is_empty()) {
        return Err("文件名不对".into());
    }
    Ok(if name.ends_with(".http") || name.ends_with(".rest") { name.to_string() } else { format!("{name}.http") })
}

fn err(e: callme_core::Error) -> String {
    e.to_json().to_string()
}

/// The project's operations, on callme-core. Results are JSON strings; errors are JSON `{ error: { kind, message,
/// hint? } }` strings.
#[module]
pub struct Callme;

#[module]
impl Callme {
    /// The project to open: `$CALLME_PROJECT`, else the one opened last (`~/.callme/last-project`, which
    /// `callme open` writes too), else the working directory unless it is `/` (an app opened from the Finder);
    /// looking upward for `http-client.env.json` or `.callme/`. Empty when there is none.
    pub fn root() -> String {
        let from_env = std::env::var_os("CALLME_PROJECT").map(PathBuf::from);
        let last = last_project().filter(|p| p.is_dir());
        let cwd = std::env::current_dir().ok().filter(|d| d.parent().is_some());
        match from_env.or(last).or(cwd) {
            Some(start) => Project::discover(&start).root.display().to_string(),
            None => String::new(),
        }
    }

    /// Remembers `root` as the project to open next time, first of the recent projects.
    pub fn remember(root: String) {
        let Some(file) = last_project_file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&file, &root);
        let mut recent = recent_projects();
        recent.retain(|p| *p != root);
        recent.insert(0, root);
        recent.truncate(8);
        let _ = std::fs::write(file.with_file_name("recent-projects.json"), serde_json::to_string_pretty(&recent).unwrap());
    }

    /// The projects opened lately, newest first, those still there, as JSON.
    pub fn recent() -> String {
        let list: Vec<String> = recent_projects().into_iter().filter(|p| std::path::Path::new(p).is_dir()).collect();
        serde_json::to_string(&list).unwrap()
    }

    /// Asks for a folder to open; resolves with its path, or null when cancelled.
    pub async fn pick_folder() -> Result<Option<String>, String> {
        let picked = pick_paths(OpenDialog::folders().title("打开项目文件夹").button_label("打开")).await?;
        Ok(picked.and_then(|p| p.into_iter().next()).map(|p| p.display().to_string()))
    }

    /// Every request in the project, as JSON.
    pub fn list(root: String) -> Result<String, String> {
        let project = Project { root: root.into() };
        project.list().map(|l| serde_json::to_string(&l).unwrap()).map_err(err)
    }

    /// A file's text.
    pub fn read(root: String, file: String) -> Result<String, String> {
        std::fs::read_to_string(PathBuf::from(root).join(&file)).map_err(|e| format!("读取 {file} 失败：{e}"))
    }

    /// Writes a file's text.
    pub fn write(root: String, file: String, text: String) -> Result<(), String> {
        let path = PathBuf::from(root).join(&file);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(path, text).map_err(|e| format!("保存 {file} 失败：{e}"))
    }

    /// The request folds of `.http` text, as JSON `[{ startLine, endLine }]`.
    pub fn http_folds(text: String) -> String {
        serde_json::to_string(&lang::http_folds(&text)).unwrap()
    }

    /// The object and array folds of JSON text, as JSON `[{ startLine, endLine }]`.
    pub fn json_folds(text: String) -> String {
        serde_json::to_string(&lang::json_folds(&text)).unwrap()
    }

    /// Runs request `id` of `file` (its only one when none) in environment `env` (the current one when none);
    /// resolves with the run's JSON. `onEvent` hears JSON messages as the response arrives:
    /// `{ type: "head", status, headers, streaming }` and `{ type: "event", event }` for each server-sent event.
    pub async fn run(
        root: String,
        file: String,
        id: Option<String>,
        env: Option<String>,
        #[args(message)] on_event: Callback<String>,
    ) -> Result<String, String> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let job = blocking(move || {
            let project = Project { root: root.into() };
            let mut forward = Forward { tx, sse: None };
            let result = run::run(&project, &file, id.as_deref(), RunOptions { env, overrides: vec![], on_chunk: Some(&mut forward) });
            result
                .map(|r| output::record_json(&r.record, Some(&r.raw_body), Some(&r.history_file), MAX_BODY).to_string())
                .map_err(err)
        });
        while let Some(message) = rx.recv().await {
            on_event.post(message);
        }
        job.await.map_err(|e| json!({ "error": { "kind": "internal", "message": e.to_string() } }).to_string())?
    }

    /// Diagnostics of `.http` text `text` (file `file`) in environment `env` (the current one when none), as JSON
    /// `[{ line, start, end, severity, message }]` in UTF-16 columns.
    pub fn diagnostics(root: String, file: String, text: String, env: Option<String>) -> String {
        let project = Project { root: root.into() };
        serde_json::to_string(&Service { project: &project, file: &file, text: &text, env }.diagnostics()).unwrap()
    }

    /// Completion items at `line`, UTF-16 `column`, as JSON `[{ label, kind, detail?, insertText? }]`.
    pub fn completions(root: String, file: String, text: String, line: u32, column: u32, env: Option<String>) -> String {
        let project = Project { root: root.into() };
        let items = Service { project: &project, file: &file, text: &text, env }.completions(line as usize, column as usize);
        serde_json::to_string(&items).unwrap()
    }

    /// What a hover over `line`, UTF-16 `column` shows, as JSON, or null over no variable.
    pub fn hover(root: String, file: String, text: String, line: u32, column: u32, env: Option<String>) -> Option<String> {
        let project = Project { root: root.into() };
        let h = Service { project: &project, file: &file, text: &text, env }.hover(line as usize, column as usize)?;
        Some(serde_json::to_string(&h).unwrap())
    }

    /// What the project's `.gitignore` lacks of `.callme/` and `http-client.private.env.json`, as JSON; empty
    /// when the project is not in git (no `.git` up from it) and has no `.gitignore`.
    pub fn gitignore_missing(root: String) -> String {
        let root = PathBuf::from(root);
        let in_git = root.ancestors().any(|d| d.join(".git").exists());
        let path = root.join(".gitignore");
        if !in_git && !path.exists() {
            return "[]".into();
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let has = |p: &str| text.lines().any(|l| { let l = l.trim(); l == p || l == p.trim_end_matches('/') || l == format!("/{p}") });
        let missing: Vec<&str> = [".callme/", callme_core::env::PRIVATE_FILE].into_iter().filter(|p| !has(p)).collect();
        serde_json::to_string(&missing).unwrap()
    }

    /// Adds `patterns` to the project's `.gitignore` (created when missing).
    pub fn add_to_gitignore(root: String, patterns: Vec<String>) -> Result<(), String> {
        let path = PathBuf::from(root).join(".gitignore");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("# callme\n");
        for p in patterns {
            text.push_str(&p);
            text.push('\n');
        }
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    /// Diagnostics of environment file `file`'s text, as JSON `[{ line, start, end, severity, message }]`.
    pub fn env_diagnostics(root: String, file: String, text: String) -> String {
        let project = Project { root: root.into() };
        serde_json::to_string(&service::env_file_diagnostics(&project, &file, &text)).unwrap()
    }

    /// `.http` text with request `index`'s title (after its `###`) set to `title`.
    pub fn set_title(text: String, index: u32, title: String) -> Result<String, String> {
        lang::set_title(&text, index as usize, &title)
    }

    /// A request name (`# @name`) made from a title: `agent 列表` → `agent-列表`.
    pub fn name_from_title(title: String) -> String {
        lang::name_from_title(&title)
    }

    /// Names request `index` of `file` (its text, unsaved, is `text`) `name`; resolves with the file's new text.
    /// References to its old name follow: in the file, and in the project's other files when no other request
    /// has that name.
    pub fn rename_request(root: String, file: String, text: String, index: u32, name: String) -> Result<String, String> {
        let project = Project { root: root.into() };
        let old = callme_core::parse::parse(&text).requests.into_iter().find(|r| r.index == index as usize).and_then(|r| r.name);
        let mut next = lang::rename_request(&text, index as usize, &name)?;
        if let Some(old) = old {
            let name = name.trim();
            next = lang::rename_references(&next, &old, name);
            let elsewhere = project.list().unwrap_or_default().into_iter().filter(|r| r.name.as_deref() == Some(&old) && r.file != file).count();
            if elsewhere == 0 {
                for f in project.http_files().into_iter().filter(|f| *f != file) {
                    let path = project.root.join(&f);
                    let Ok(t) = std::fs::read_to_string(&path) else { continue };
                    let changed = lang::rename_references(&t, &old, name);
                    if changed != t {
                        std::fs::write(&path, changed).map_err(|e| format!("改 {f} 里的引用失败：{e}"))?;
                    }
                }
            }
        }
        Ok(next)
    }

    /// `.http` text with request `index`'s JSON body pretty-printed; null when it has no body to format.
    pub fn format_body(text: String, index: u32) -> Result<Option<String>, String> {
        lang::format_body(&text, index as usize)
    }

    /// Where the `{{…}}` at `line`, UTF-16 `column` is defined, as JSON `{ file, line, start, end }`, or null.
    pub fn definition(root: String, file: String, text: String, line: u32, column: u32, env: Option<String>) -> Option<String> {
        let project = Project { root: root.into() };
        let d = Service { project: &project, file: &file, text: &text, env }.definition(line as usize, column as usize)?;
        Some(serde_json::to_string(&d).unwrap())
    }

    /// The method and URL request `index` would be sent with, as JSON `{ method, url }` (secrets masked).
    pub fn preview(root: String, file: String, text: String, index: u32, env: Option<String>) -> Result<String, String> {
        let project = Project { root: root.into() };
        let (method, url) = Service { project: &project, file: &file, text: &text, env }.preview(index as usize).map_err(err)?;
        Ok(json!({ "method": method, "url": url }).to_string())
    }

    /// Request `index` as a curl command.
    pub fn curl(root: String, file: String, text: String, index: u32, env: Option<String>) -> Result<String, String> {
        let project = Project { root: root.into() };
        Service { project: &project, file: &file, text: &text, env }.curl(index as usize).map_err(err)
    }

    /// The 1-based index of the request at `line` of `.http` text, 0 for none.
    pub fn request_at(text: String, line: u32) -> u32 {
        lang::request_at(&text, line as usize).unwrap_or(0) as u32
    }

    /// The 0-based line of request `index`'s request line (its method and URL), 0 when there is none.
    pub fn request_line(text: String, index: u32) -> u32 {
        callme_core::parse::parse(&text).requests.iter().find(|r| r.index == index as usize).map_or(0, |r| r.request_line as u32)
    }

    /// The `{{…}}` of a text, as JSON `[{ line, start, end }]` in UTF-16 columns.
    pub fn variables(text: String) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let out: Vec<Value> = lang::all_variables(&text)
            .into_iter()
            .map(|(line, r, _)| {
                let src = lines[line];
                json!({ "line": line, "start": lang::utf16_col(src, r.start), "end": lang::utf16_col(src, r.end) })
            })
            .collect();
        Value::Array(out).to_string()
    }

    /// The environments and one environment's variables, as JSON `{ envs, current, env, vars: [{ name, value,
    /// source }] }`, secrets masked.
    pub fn env_view(root: String, env: Option<String>) -> Result<String, String> {
        let project = Project { root: root.into() };
        service::env_view(&project, env.as_deref()).map(|v| serde_json::to_string(&v).unwrap()).map_err(err)
    }

    /// Makes `env` the current environment.
    pub fn set_env(root: String, env: String) -> Result<(), String> {
        run::set_current_env(&Project { root: root.into() }, &env).map_err(err)
    }

    /// The newest `limit` runs of request `id` of `file`, as JSON, newest first.
    pub fn history(root: String, file: String, id: String, limit: u32) -> String {
        let project = Project { root: root.into() };
        let items: Vec<Value> = history::list(&project, &file, &id, limit as usize)
            .iter()
            .map(|(p, r)| output::record_json(r, None, Some(p), MAX_BODY))
            .collect();
        Value::Array(items).to_string()
    }

    /// What JSONPath `path` selects in JSON `json`, as a JSON array.
    pub fn filter(json: String, path: String) -> Result<String, String> {
        let value: Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        callme_core::resolve::query_all(&value, &path).map(|v| Value::Array(v).to_string()).map_err(|e| e.to_string())
    }

    /// A curl command or `fetch(…)` call as `.http` text, with `env`'s variables in place of its values, as JSON
    /// `{ text, replaced }`.
    pub fn import(root: String, input: String, env: Option<String>, name: Option<String>) -> Result<String, String> {
        let project = Project { root: root.into() };
        let imported = import::parse_any(&input).map_err(err)?;
        let env = env.or_else(|| run::current_env(&project));
        let vars = project.env_files().ok().and_then(|f| f.resolve(env.as_deref(), &[]).ok());
        let c = import::to_http(&imported, vars.as_ref(), name.as_deref());
        Ok(json!({ "text": c.text, "replaced": c.replaced }).to_string())
    }

    /// Adds environment `name` to `http-client.env.json` (created when missing), with the variable names of
    /// environment `like` and empty values to fill in.
    pub fn add_env(root: String, name: String, like: Option<String>) -> Result<(), String> {
        let name = name.trim().to_string();
        if name.is_empty() || name == "$shared" || name.contains(char::is_whitespace) {
            return Err("环境名不能为空、不能有空格，也不能是 $shared".into());
        }
        let path = PathBuf::from(&root).join(callme_core::env::PUBLIC_FILE);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
        let mut doc: Value = serde_json::from_str(if text.trim().is_empty() { "{}" } else { &text })
            .map_err(|e| format!("{} 不是合法的 JSON：{e}", callme_core::env::PUBLIC_FILE))?;
        let envs = doc.as_object_mut().ok_or("http-client.env.json 的顶层应该是对象")?;
        if envs.contains_key(&name) {
            return Err(format!("已经有环境 `{name}` 了"));
        }
        let vars: serde_json::Map<String, Value> = like
            .and_then(|l| envs.get(&l).and_then(Value::as_object).cloned())
            .map(|m| m.keys().map(|k| (k.clone(), Value::String(String::new()))).collect())
            .unwrap_or_default();
        envs.insert(name.clone(), Value::Object(vars));
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").map_err(|e| e.to_string())?;
        run::set_current_env(&Project { root: root.into() }, &name).map_err(err)
    }

    /// Renames file `from` to `to` (both relative to the root; `.http` added when `to` has no extension).
    pub fn rename(root: String, from: String, to: String) -> Result<String, String> {
        let to = normalize_http(&to)?;
        let (a, b) = (PathBuf::from(&root).join(&from), PathBuf::from(&root).join(&to));
        if b.exists() {
            return Err(format!("{to} 已经存在"));
        }
        if let Some(dir) = b.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::rename(a, b).map_err(|e| format!("重命名失败：{e}"))?;
        Ok(to)
    }

    /// Creates `.http` file `file` (`.http` added when it has no extension) with a request to start from;
    /// resolves with its path.
    pub fn create(root: String, file: String) -> Result<String, String> {
        let file = normalize_http(&file)?;
        let path = PathBuf::from(&root).join(&file);
        if path.exists() {
            return Err(format!("{file} 已经存在"));
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&path, "### 新请求\n# @name request\nGET {{baseUrl}}/\nAccept: application/json\n").map_err(|e| e.to_string())?;
        Ok(file)
    }

    /// Moves `file` to the Trash.
    pub async fn trash(root: String, file: String) -> Result<(), String> {
        move_to_trash(PathBuf::from(root).join(file)).await.map(|_| ())
    }

    /// Shows `file` in the Finder.
    pub async fn reveal(root: String, file: String) -> Result<(), String> {
        reveal_path(PathBuf::from(root).join(file)).await
    }

    /// The `.http` files of the project, as JSON.
    pub fn files(root: String) -> String {
        serde_json::to_string(&Project { root: root.into() }.http_files()).unwrap()
    }

    /// When `file` last changed, in milliseconds since the epoch (0 when it is missing).
    pub fn modified(root: String, file: String) -> f64 {
        std::fs::metadata(PathBuf::from(root).join(file))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_millis() as f64)
    }

    /// Pretty JSON of a value given as JSON (the response body as the editor shows it).
    pub fn pretty(json: String) -> String {
        serde_json::from_str::<Value>(&json).map(|v| serde_json::to_string_pretty(&v).unwrap()).unwrap_or(json)
    }
}
