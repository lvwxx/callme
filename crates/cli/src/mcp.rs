//! `callme mcp`: an MCP server on stdio (JSON-RPC 2.0, one message per line). Tools read and run requests; none
//! writes files, since agents edit `.http` files with their own tools. Results are the CLI's JSON.

use std::io::{BufRead, Write};

use callme_core::env::Source;
use callme_core::history;
use callme_core::output::{self, DEFAULT_MAX_BODY};
use callme_core::project::Project;
use callme_core::run::{self, RunOptions};
use callme_core::Error;
use serde_json::{json, Value};

const PROTOCOL: &str = "2025-06-18";

pub fn serve(project: &Project) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(project, &msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})),
        };
        if let Some(r) = response {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}

pub fn handle(project: &Project, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    // Notifications get no answer.
    let id = id?;
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "callme", "version": env!("CARGO_PKG_VERSION") },
            "instructions": format!(
                "callme runs requests written in .http files in the project at {}. Edit .http files with your own file tools; \
                 use these tools to list, run and inspect them.\n\n{}",
                project.root.display(),
                crate::FORMAT_GUIDE
            ),
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => Ok(call(project, &params)),
        _ => Err((-32601, format!("unknown method {method}"))),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "list_requests",
            "description": "List every request in the project's .http files: file, name, index, method and URL (variables unresolved).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "run_request",
            "description": "Execute a request and return the response (status, headers, body, timing). JSON bodies are embedded; bodies over max_body bytes are cut, the whole body is in historyFile. Secrets are masked.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path of the .http file, relative to the project root" },
                    "name": { "type": "string", "description": "Request name (# @name) or 1-based index; optional when the file has one request" },
                    "env": { "type": "string", "description": "Environment; default: the one last selected" },
                    "vars": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Variables overriding the environment's" },
                    "max_body": { "type": "integer", "description": "Default 32768" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "get_history",
            "description": "The most recent responses of a request, newest first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": { "type": "string" },
                    "name": { "type": "string" },
                    "limit": { "type": "integer", "description": "Default 5" },
                    "max_body": { "type": "integer" }
                },
                "required": ["file"]
            }
        },
        {
            "name": "list_envs",
            "description": "The environments, the current one, and the variables of one environment with their sources (secrets masked).",
            "inputSchema": {
                "type": "object",
                "properties": { "env": { "type": "string", "description": "Default: the current environment" } }
            }
        }
    ])
}

fn call(project: &Project, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
    let n = |k: &str, d: usize| args.get(k).and_then(Value::as_u64).map_or(d, |v| v as usize);
    let result: Result<Value, Error> = (|| match name {
        "list_requests" => Ok(serde_json::to_value(project.list()?).unwrap()),
        "run_request" => {
            let file = s("file").ok_or_else(|| Error::Usage("缺少参数 file".into()))?;
            let file = project.relative(&project.root.join(&file));
            let overrides = match args.get("vars") {
                Some(Value::Object(m)) => {
                    m.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect()
                }
                _ => vec![],
            };
            let r = run::run(project, &file, s("name").as_deref(), RunOptions { env: s("env"), overrides, on_chunk: None })?;
            Ok(output::record_json(&r.record, Some(&r.raw_body), Some(&r.history_file), n("max_body", DEFAULT_MAX_BODY)))
        }
        "get_history" => {
            let file = s("file").ok_or_else(|| Error::Usage("缺少参数 file".into()))?;
            let file = project.relative(&project.root.join(&file));
            let (_, request) = project.request(&file, s("name").as_deref())?;
            let records = history::list(project, &file, &request.id(), n("limit", 5));
            Ok(Value::Array(
                records.iter().map(|(p, r)| output::record_json(r, None, Some(p), n("max_body", DEFAULT_MAX_BODY))).collect(),
            ))
        }
        "list_envs" => {
            let files = project.env_files()?;
            let current = run::current_env(project);
            let shown = s("env").or_else(|| current.clone());
            let vars = files.resolve(shown.as_deref(), &[])?;
            let vars_json: serde_json::Map<String, Value> = vars
                .vars
                .iter()
                .map(|(k, v)| {
                    let value = if v.source == Source::Private { "***".into() } else { v.value.clone() };
                    (k.clone(), json!({ "value": value, "source": v.source }))
                })
                .collect();
            Ok(json!({ "envs": files.names(), "current": current, "env": shown, "vars": vars_json }))
        }
        other => Err(Error::Usage(format!("没有工具 `{other}`"))),
    })();
    match result {
        Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap() }], "isError": false }),
        Err(e) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&e.to_json()).unwrap() }], "isError": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_the_handshake_and_lists_tools() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project { root: dir.path().to_path_buf() };
        std::fs::write(p.root.join("a.http"), "# @name ping\nGET https://x.dev\n").unwrap();

        let init = handle(&p, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(&p, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());

        let list = handle(&p, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 4);

        let call = handle(&p, &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "list_requests", "arguments": {}}})).unwrap();
        assert_eq!(call["result"]["isError"], false);
        assert!(call["result"]["content"][0]["text"].as_str().unwrap().contains("\"ping\""));

        let bad = handle(&p, &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "run_request", "arguments": {"file": "a.http", "name": "nope"}}})).unwrap();
        assert_eq!(bad["result"]["isError"], true);
    }
}
