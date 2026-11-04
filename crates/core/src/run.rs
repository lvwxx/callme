//! Running a request end to end: pick the environment, resolve, execute, classify the body, write history.

use serde_json::Value;

use crate::env::Vars;
use crate::exec;
use crate::history::{self, BodyKind, ReceivedResponse, Record, SentRequest};
use crate::project::Project;
use crate::resolve::Context;
use crate::Error;

#[derive(Default)]
pub struct RunOptions<'a> {
    /// The environment; none: the one last selected (`.callme/state.json`).
    pub env: Option<String>,
    pub overrides: Vec<(String, String)>,
    pub on_chunk: exec::OnChunk<'a>,
}

pub struct RunResult {
    pub record: Record,
    pub history_file: String,
    pub raw_body: Vec<u8>,
    pub vars: Vars,
}

pub fn run(project: &Project, file: &str, id: Option<&str>, opts: RunOptions<'_>) -> Result<RunResult, Error> {
    let (_, request) = project.request(file, id)?;
    let env = opts.env.or_else(|| current_env(project));
    let vars = project.env_files()?.resolve(env.as_deref(), &opts.overrides)?;
    let cx = Context { project, file, vars: &vars };
    let resolved = cx.prepare(&request, Some(project.state_dir().join("cookies.txt")))?;
    let response = exec::execute(&resolved.prepared, opts.on_chunk)?;

    let content_type = response
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default();
    let (body_kind, body) = classify(&content_type, &response.body);
    let p = &resolved.prepared;
    let mut record = Record {
        at: history::now(),
        file: file.to_string(),
        id: request.id(),
        env: vars.env.clone(),
        request: SentRequest {
            method: p.method.clone(),
            url: vars.mask(&p.url),
            headers: p.headers.iter().map(|(n, v)| (n.clone(), vars.mask(v))).collect(),
            added_headers: resolved.added_headers,
        },
        response: ReceivedResponse {
            status: response.status,
            reason: response.reason,
            http_version: response.http_version,
            headers: response.headers,
            size: response.body.len(),
            body_kind,
            body,
            body_file: None,
        },
        timing: response.timing,
    };
    let history_file = history::write(project, &mut record, &response.body)?;
    Ok(RunResult { record, history_file, raw_body: response.body, vars })
}

/// The body as history keeps it: JSON parsed, server-sent events split, text as a string.
pub fn classify(content_type: &str, body: &[u8]) -> (BodyKind, Value) {
    if body.is_empty() {
        return (BodyKind::Empty, Value::Null);
    }
    if content_type.starts_with("text/event-stream") {
        if let Ok(text) = std::str::from_utf8(body) {
            return (BodyKind::Sse, Value::Array(parse_sse(text)));
        }
    }
    if content_type.contains("json") || content_type.is_empty() || content_type.starts_with("text/") {
        if let Ok(v) = serde_json::from_slice::<Value>(body) {
            return (BodyKind::Json, v);
        }
    }
    match std::str::from_utf8(body) {
        Ok(text) => (BodyKind::Text, Value::String(text.to_string())),
        Err(_) => (BodyKind::Binary, Value::Null),
    }
}

/// Server-sent events: `{event?, id?, data}`, `data` parsed when it is JSON.
pub fn parse_sse(text: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let mut event = serde_json::Map::new();
        let mut data: Vec<&str> = Vec::new();
        for line in block.lines() {
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').map(|(f, v)| (f, v.strip_prefix(' ').unwrap_or(v))).unwrap_or((line, ""));
            match field {
                "data" => data.push(value),
                "event" | "id" | "retry" => {
                    event.insert(field.to_string(), Value::String(value.to_string()));
                }
                _ => {}
            }
        }
        if data.is_empty() && event.is_empty() {
            continue;
        }
        let data = data.join("\n");
        let data = serde_json::from_str::<Value>(&data).unwrap_or(Value::String(data));
        event.insert("data".into(), data);
        events.push(Value::Object(event));
    }
    events
}

fn state_path(project: &Project) -> std::path::PathBuf {
    project.state_dir().join("state.json")
}

/// The environment last selected in this project.
pub fn current_env(project: &Project) -> Option<String> {
    let text = std::fs::read_to_string(state_path(project)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("env")?.as_str().map(str::to_string)
}

pub fn set_current_env(project: &Project, env: &str) -> Result<(), Error> {
    let path = state_path(project);
    let mut state: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    state["env"] = env.into();
    std::fs::create_dir_all(project.state_dir()).map_err(|e| Error::Io(project.state_dir().display().to_string(), e))?;
    std::fs::write(&path, serde_json::to_vec_pretty(&state).unwrap()).map_err(|e| Error::Io(path.display().to_string(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_bodies() {
        assert_eq!(classify("application/json; charset=utf-8", b"{\"a\":1}").0, BodyKind::Json);
        assert_eq!(classify("text/plain", b"hello").0, BodyKind::Text);
        assert_eq!(classify("application/octet-stream", &[0xff, 0xfe]).0, BodyKind::Binary);
        assert_eq!(classify("application/json", b"").0, BodyKind::Empty);
        assert_eq!(classify("application/json", b"not json").0, BodyKind::Text);
    }

    #[test]
    fn parses_server_sent_events() {
        let events = parse_sse(": comment\nevent: delta\ndata: {\"t\": \"he\"}\n\nid: 2\ndata: line1\ndata: line2\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["event"], "delta");
        assert_eq!(events[0]["data"]["t"], "he");
        assert_eq!(events[1]["data"], "line1\nline2");
        assert_eq!(events[1]["id"], "2");
    }

    #[test]
    fn remembers_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project { root: dir.path().to_path_buf() };
        assert_eq!(current_env(&p), None);
        set_current_env(&p, "staging").unwrap();
        assert_eq!(current_env(&p).as_deref(), Some("staging"));
    }
}
