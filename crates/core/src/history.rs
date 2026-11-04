//! History: one JSON file per execution, at `.callme/history/<file>/<request id>/<time>.json`, the newest 50 kept
//! per request. The request is stored masked (secrets as `***`), the response as received. A body over 10 MB is
//! kept in a file beside the record.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::exec::Timing;
use crate::project::Project;
use crate::Error;

pub const KEEP: usize = 50;
pub const MAX_INLINE_BODY: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub at: String,
    pub file: String,
    pub id: String,
    pub env: Option<String>,
    pub request: SentRequest,
    pub response: ReceivedResponse,
    pub timing: Timing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Headers callme added (`Content-Type` for a JSON body), not written in the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_headers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedResponse {
    pub status: u16,
    pub reason: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    pub size: usize,
    pub body_kind: BodyKind,
    /// A JSON value for `json`, an array of events for `sse`, a string for `text`, null for `binary` or a body kept
    /// in `bodyFile`.
    pub body: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyKind {
    Json,
    Sse,
    Text,
    Binary,
    Empty,
}

impl ReceivedResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn dir(project: &Project, file: &str, id: &str) -> PathBuf {
    project.state_dir().join("history").join(file).join(id)
}

/// Writes `record`, moving a large body out of it into `body` beside it; returns the record's path relative to the
/// project root.
pub fn write(project: &Project, record: &mut Record, raw_body: &[u8]) -> Result<String, Error> {
    let dir = dir(project, &record.file, &record.id);
    std::fs::create_dir_all(&dir).map_err(|e| Error::Io(dir.display().to_string(), e))?;
    let stamp: String = record.at.chars().filter(char::is_ascii_alphanumeric).collect();
    if raw_body.len() > MAX_INLINE_BODY {
        let body_path = dir.join(format!("{stamp}.body"));
        std::fs::write(&body_path, raw_body).map_err(|e| Error::Io(body_path.display().to_string(), e))?;
        record.response.body = Value::Null;
        record.response.body_file = Some(project.relative(&body_path));
    }
    let path = dir.join(format!("{stamp}.json"));
    let json = serde_json::to_vec_pretty(record).expect("a record serializes");
    std::fs::write(&path, json).map_err(|e| Error::Io(path.display().to_string(), e))?;
    prune(&dir);
    Ok(project.relative(&path))
}

/// The newest `limit` records of a request, newest first, with their paths.
pub fn list(project: &Project, file: &str, id: &str, limit: usize) -> Vec<(String, Record)> {
    let dir = dir(project, file, id);
    entries(&dir)
        .into_iter()
        .rev()
        .take(limit)
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let record: Record = serde_json::from_str(&text).ok()?;
            Some((project.relative(&p), record))
        })
        .collect()
}

pub fn latest(project: &Project, file: &str, id: &str) -> Option<Record> {
    list(project, file, id, 1).pop().map(|(_, r)| r)
}

/// Reads a record's whole body: inline, or from its body file.
pub fn full_body(project: &Project, record: &Record) -> Option<Vec<u8>> {
    match &record.response.body_file {
        Some(f) => std::fs::read(project.root.join(f)).ok(),
        None => match &record.response.body {
            Value::Null => None,
            Value::String(s) => Some(s.clone().into_bytes()),
            v => Some(serde_json::to_vec(v).unwrap_or_default()),
        },
    }
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    v.sort();
    v
}

fn prune(dir: &Path) {
    let all = entries(dir);
    if all.len() > KEEP {
        for old in &all[..all.len() - KEEP] {
            let _ = std::fs::remove_file(old);
            let _ = std::fs::remove_file(old.with_extension("body"));
        }
    }
}

pub fn now() -> String {
    humantime::format_rfc3339_millis(std::time::SystemTime::now()).to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn record(file: &str, id: &str, at: &str, body: Value) -> Record {
        Record {
            at: at.into(),
            file: file.into(),
            id: id.into(),
            env: Some("dev".into()),
            request: SentRequest { method: "GET".into(), url: "https://x.dev".into(), headers: vec![], added_headers: vec![] },
            response: ReceivedResponse {
                status: 200,
                reason: "OK".into(),
                http_version: "HTTP/2".into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                size: 2,
                body_kind: BodyKind::Json,
                body,
                body_file: None,
            },
            timing: Timing::default(),
        }
    }

    #[test]
    fn writes_lists_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project { root: dir.path().to_path_buf() };
        for i in 0..(KEEP + 3) {
            let mut r = record("a.http", "login", &format!("2026-11-04T06:32:{:02}.000Z", i), serde_json::json!({"i": i}));
            write(&p, &mut r, b"{}").unwrap();
        }
        let all = list(&p, "a.http", "login", 100);
        assert_eq!(all.len(), KEEP);
        assert_eq!(all[0].1.response.body["i"], KEEP + 2, "newest first");
        assert!(all[0].0.starts_with(".callme/history/a.http/login/"));
        assert_eq!(latest(&p, "a.http", "login").unwrap().response.body["i"], KEEP + 2);
        assert!(latest(&p, "a.http", "other").is_none());
    }

    #[test]
    fn large_bodies_go_beside_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project { root: dir.path().to_path_buf() };
        let big = vec![b'a'; MAX_INLINE_BODY + 1];
        let mut r = record("a.http", "#1", "2026-11-04T06:32:00.000Z", Value::String("x".into()));
        write(&p, &mut r, &big).unwrap();
        let back = latest(&p, "a.http", "#1").unwrap();
        assert_eq!(back.response.body, Value::Null);
        assert_eq!(full_body(&p, &back).unwrap().len(), big.len());
    }
}
