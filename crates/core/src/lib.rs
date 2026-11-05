//! callme's core: everything the CLI, the MCP server and the GUI share. Parsing `.http` files, environments,
//! resolving variables and response references, executing requests with libcurl, history, and importing curl /
//! fetch snippets. Nothing here knows about a user interface.

pub mod env;
pub mod exec;
pub mod history;
pub mod import;
pub mod lang;
pub mod output;
pub mod parse;
pub mod project;
pub mod resolve;
pub mod run;
pub mod service;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Something the user asked for that cannot be done as asked: an unknown request or environment, an undefined
    /// variable, a file that does not parse.
    #[error("{0}")]
    Usage(String),
    #[error("{file}:{line}: {message}")]
    Parse { file: String, line: usize, message: String },
    #[error("读取 {0} 失败：{1}")]
    Io(String, #[source] std::io::Error),
    #[error("{message}")]
    Request { kind: RequestErrorKind, message: String, hint: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestErrorKind {
    Network,
    Timeout,
    Tls,
    /// Stopped by the user before a response arrived.
    Cancelled,
}

impl Error {
    /// The `kind` of the error in JSON output.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Usage(_) => "usage",
            Error::Parse { .. } => "parse",
            Error::Io(..) => "io",
            Error::Request { kind: RequestErrorKind::Network, .. } => "network",
            Error::Request { kind: RequestErrorKind::Timeout, .. } => "timeout",
            Error::Request { kind: RequestErrorKind::Tls, .. } => "tls",
            Error::Request { kind: RequestErrorKind::Cancelled, .. } => "cancelled",
        }
    }

    pub fn hint(&self) -> Option<&str> {
        match self {
            Error::Request { hint, .. } => hint.as_deref(),
            _ => None,
        }
    }

    /// The process exit code: 1 when the request could not be made, 2 when the input is wrong.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Request { .. } => 1,
            _ => 2,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut e = serde_json::json!({ "kind": self.kind(), "message": self.to_string() });
        if let Some(h) = self.hint() {
            e["hint"] = h.into();
        }
        serde_json::json!({ "error": e })
    }
}
