//! The JSON shape the CLI (non-terminal output) and the MCP server give a run or a history record: secrets masked,
//! JSON bodies embedded as values, bodies over `max_body` bytes cut (the whole body stays in the history file).

use serde_json::{json, Map, Value};

use crate::history::{BodyKind, Record};

pub const DEFAULT_MAX_BODY: usize = 32 * 1024;

/// Headers as an object; a header sent several times (`set-cookie`) becomes an array.
pub fn headers_object(headers: &[(String, String)]) -> Value {
    let mut map = Map::new();
    for (n, v) in headers {
        let key = n.to_ascii_lowercase();
        match map.get_mut(&key) {
            None => {
                map.insert(key, Value::String(v.clone()));
            }
            Some(Value::Array(a)) => a.push(Value::String(v.clone())),
            Some(prev) => {
                let first = prev.take();
                *prev = Value::Array(vec![first, Value::String(v.clone())]);
            }
        }
    }
    Value::Object(map)
}

pub fn record_json(record: &Record, raw_body: Option<&[u8]>, history_file: Option<&str>, max_body: usize) -> Value {
    let r = &record.response;
    let (body, truncated) = if r.size > max_body {
        let text = match raw_body {
            Some(b) => String::from_utf8_lossy(&b[..max_body.min(b.len())]).into_owned(),
            None => match &r.body {
                Value::String(s) => s.chars().take(max_body).collect(),
                Value::Null => String::new(),
                v => v.to_string().chars().take(max_body).collect(),
            },
        };
        (if r.body_kind == BodyKind::Binary { Value::Null } else { Value::String(text) }, true)
    } else {
        (r.body.clone(), false)
    };
    let mut response = json!({
        "status": r.status,
        "reason": r.reason,
        "httpVersion": r.http_version,
        "headers": headers_object(&r.headers),
        "bodyKind": r.body_kind,
        "body": body,
        "bodyTruncated": truncated,
        "size": r.size,
    });
    if let Some(f) = &r.body_file {
        response["bodyFile"] = f.as_str().into();
    }
    let mut request = json!({
        "file": record.file,
        "id": record.id,
        "env": record.env,
        "method": record.request.method,
        "url": record.request.url,
        "headers": headers_object(&record.request.headers),
    });
    if !record.request.added_headers.is_empty() {
        request["addedHeaders"] = json!(record.request.added_headers);
    }
    let mut out = json!({
        "at": record.at,
        "request": request,
        "response": response,
        "timing": record.timing,
    });
    if let Some(f) = history_file {
        out["historyFile"] = f.into();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::tests::record;

    #[test]
    fn repeated_headers_become_arrays() {
        let h = headers_object(&[("Set-Cookie".into(), "a=1".into()), ("set-cookie".into(), "b=2".into()), ("X".into(), "y".into())]);
        assert_eq!(h, json!({"set-cookie": ["a=1", "b=2"], "x": "y"}));
    }

    #[test]
    fn truncates_large_bodies() {
        let mut r = record("a.http", "#1", "t", json!({"a": "x".repeat(100)}));
        r.response.size = 110;
        let raw = serde_json::to_vec(&r.response.body).unwrap();
        let out = record_json(&r, Some(&raw), Some("h.json"), 10);
        assert_eq!(out["response"]["bodyTruncated"], true);
        assert_eq!(out["response"]["body"].as_str().unwrap().len(), 10);
        assert_eq!(out["historyFile"], "h.json");

        let out = record_json(&r, Some(&raw), None, 1000);
        assert_eq!(out["response"]["body"]["a"].as_str().unwrap().len(), 100);
    }
}
