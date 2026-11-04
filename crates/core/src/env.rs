//! Environments: `http-client.env.json` (shared, committed) and `http-client.private.env.json` (secrets, ignored),
//! in the JetBrains format. Each top-level key is an environment; `$shared` applies to all of them.
//!
//! Precedence, lowest first: public `$shared`, private `$shared`, public environment, private environment, then
//! overrides (`--var`). Every value that comes from the private file is a secret and is masked in output.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::Error;

pub const PUBLIC_FILE: &str = "http-client.env.json";
pub const PRIVATE_FILE: &str = "http-client.private.env.json";
const SHARED: &str = "$shared";

/// Environment name → variable name → value.
type EnvMap = BTreeMap<String, BTreeMap<String, String>>;

#[derive(Debug, Clone, Default)]
pub struct EnvFiles {
    public: EnvMap,
    private: EnvMap,
}

/// Where a variable's value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Shared,
    Env,
    Private,
    Override,
}

#[derive(Debug, Clone, Serialize)]
pub struct Var {
    pub value: String,
    pub source: Source,
}

/// The variables of one environment, merged.
#[derive(Debug, Clone, Default)]
pub struct Vars {
    pub env: Option<String>,
    pub vars: BTreeMap<String, Var>,
    /// Values from the private file, longest first, to mask in output.
    secrets: Vec<String>,
}

impl EnvFiles {
    pub fn load(root: &Path) -> Result<Self, Error> {
        Ok(Self { public: read(&root.join(PUBLIC_FILE))?, private: read(&root.join(PRIVATE_FILE))? })
    }

    pub fn from_json(public: &str, private: &str) -> Result<Self, Error> {
        Ok(Self { public: parse(public, PUBLIC_FILE)?, private: parse(private, PRIVATE_FILE)? })
    }

    /// The environments either file defines, `$shared` aside.
    pub fn names(&self) -> Vec<String> {
        let names: BTreeSet<&String> = self.public.keys().chain(self.private.keys()).filter(|k| *k != SHARED).collect();
        names.into_iter().cloned().collect()
    }

    pub fn resolve(&self, env: Option<&str>, overrides: &[(String, String)]) -> Result<Vars, Error> {
        if let Some(e) = env {
            if !self.names().iter().any(|n| n == e) {
                return Err(Error::Usage(format!(
                    "没有环境 `{e}`，可用的环境：{}",
                    if self.names().is_empty() { "（无）".into() } else { self.names().join(", ") }
                )));
            }
        }
        let mut out = Vars { env: env.map(str::to_string), ..Default::default() };
        let mut put = |map: Option<&BTreeMap<String, String>>, source: Source| {
            for (k, v) in map.into_iter().flatten() {
                out.vars.insert(k.clone(), Var { value: v.clone(), source });
            }
        };
        put(self.public.get(SHARED), Source::Shared);
        put(self.private.get(SHARED), Source::Private);
        if let Some(e) = env {
            put(self.public.get(e), Source::Env);
            put(self.private.get(e), Source::Private);
        }
        for (k, v) in overrides {
            out.vars.insert(k.clone(), Var { value: v.clone(), source: Source::Override });
        }
        let mut secrets: Vec<String> = out
            .vars
            .values()
            .filter(|v| v.source == Source::Private && !v.value.is_empty())
            .map(|v| v.value.clone())
            .collect();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        out.secrets = secrets;
        Ok(out)
    }
}

impl Vars {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(|v| v.value.as_str())
    }

    pub fn is_secret(&self, name: &str) -> bool {
        self.vars.get(name).is_some_and(|v| v.source == Source::Private)
    }

    pub fn secrets(&self) -> &[String] {
        &self.secrets
    }

    /// Replaces every secret value in `text` with `***`.
    pub fn mask(&self, text: &str) -> String {
        let mut out = text.to_string();
        for s in &self.secrets {
            out = out.replace(s.as_str(), "***");
        }
        out
    }
}

fn read(path: &Path) -> Result<EnvMap, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, &path.display().to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(Error::Io(path.display().to_string(), e)),
    }
}

fn parse(text: &str, file: &str) -> Result<EnvMap, Error> {
    if text.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|e| Error::Usage(format!("{file} 第 {} 行不是合法的 JSON：{e}", e.line())))?;
    let Value::Object(envs) = value else {
        return Err(Error::Usage(format!("{file} 的顶层应该是对象，每个键是一个环境")));
    };
    let mut out = BTreeMap::new();
    for (env, vars) in envs {
        let Value::Object(vars) = vars else {
            return Err(Error::Usage(format!("{file} 里的环境 `{env}` 应该是对象")));
        };
        let vars = vars
            .into_iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) => Some((k, s)),
                Value::Number(_) | Value::Bool(_) => Some((k, v.to_string())),
                // Objects (JetBrains' SSL settings and the like) and nulls are not variables.
                _ => None,
            })
            .collect();
        out.insert(env, vars);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> EnvFiles {
        EnvFiles::from_json(
            r#"{"$shared": {"apiVersion": "v1", "user": "shared"},
                "dev": {"baseUrl": "https://dev.example.com", "user": "ada", "port": 8080},
                "prod": {"baseUrl": "https://example.com"}}"#,
            r#"{"dev": {"password": "s3cret-pass", "user": "private-ada"}, "local": {"token": "t"}}"#,
        )
        .unwrap()
    }

    #[test]
    fn merges_with_precedence() {
        let v = files().resolve(Some("dev"), &[("baseUrl".into(), "http://localhost".into())]).unwrap();
        assert_eq!(v.get("apiVersion"), Some("v1"));
        assert_eq!(v.get("user"), Some("private-ada"), "private beats env beats shared");
        assert_eq!(v.get("port"), Some("8080"));
        assert_eq!(v.get("baseUrl"), Some("http://localhost"), "override wins");
        assert!(v.is_secret("password"));
        assert_eq!(v.mask("u=private-ada&p=s3cret-pass"), "u=***&p=***");
    }

    #[test]
    fn lists_environments_from_both_files() {
        assert_eq!(files().names(), vec!["dev", "local", "prod"]);
    }

    #[test]
    fn unknown_environment_is_an_error() {
        let err = files().resolve(Some("staging"), &[]).unwrap_err();
        assert!(err.to_string().contains("dev, local, prod"));
    }

    #[test]
    fn no_environment_uses_shared_only() {
        let v = files().resolve(None, &[]).unwrap();
        assert_eq!(v.get("user"), Some("shared"));
        assert_eq!(v.get("baseUrl"), None);
    }

    #[test]
    fn invalid_json_names_the_line() {
        let err = EnvFiles::from_json("{\n  \"dev\": {,}\n}", "").unwrap_err();
        assert!(err.to_string().contains("第 2 行"), "{err}");
    }
}
