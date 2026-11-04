//! callme's native side: what `App.tsx` imports from `callme/native`. The `.http` language and its highlighter for
//! the editor, and `Callme`, the project operations (list, read, write, run) on callme-core. Requests run on a
//! blocking thread with libcurl; what arrives while they run (each response head, server-sent events) is posted to
//! the TSX callback as JSON.

use std::path::PathBuf;
use std::time::Duration;

use callme_core::exec::Watcher;
use callme_core::lang::{self, Token};
use callme_core::output;
use callme_core::project::Project;
use callme_core::run::{self, RunOptions, SseDecoder};
use nsh_base::editor::{
    register_highlighter, register_language, EditorHighlighter, EditorLanguage, EditorText, EditorTextEdit, EditorToken,
    EditorTokens,
};
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

fn err(e: callme_core::Error) -> String {
    e.to_json().to_string()
}

/// The project's operations, on callme-core. Results are JSON strings; errors are JSON `{ error: { kind, message,
/// hint? } }` strings.
#[module]
pub struct Callme;

#[module]
impl Callme {
    /// The project's root: `$CALLME_PROJECT`, else the working directory, looking upward for
    /// `http-client.env.json` or `.callme/`.
    pub fn root() -> String {
        let start = std::env::var_os("CALLME_PROJECT").map(PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        Project::discover(&start).root.display().to_string()
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
        std::fs::write(PathBuf::from(root).join(&file), text).map_err(|e| format!("保存 {file} 失败：{e}"))
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

    /// Pretty JSON of a value given as JSON (the response body as the editor shows it).
    pub fn pretty(json: String) -> String {
        serde_json::from_str::<Value>(&json).map(|v| serde_json::to_string_pretty(&v).unwrap()).unwrap_or(json)
    }
}
