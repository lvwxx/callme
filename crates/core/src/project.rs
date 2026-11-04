//! A project is a folder: the nearest one up from the working directory holding `http-client.env.json` or
//! `.callme/`, else the working directory itself. Requests are addressed as `<path>#<name or index>`, the path
//! relative to the root.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::env::{EnvFiles, PUBLIC_FILE};
use crate::parse::{self, HttpFile, Request};
use crate::Error;

pub const STATE_DIR: &str = ".callme";

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
}

/// One request in the project, as `callme ls` lists it.
#[derive(Debug, Clone, Serialize)]
pub struct RequestRef {
    pub file: String,
    pub name: Option<String>,
    pub index: usize,
    pub id: String,
    pub method: String,
    pub url: String,
    pub title: Option<String>,
}

impl Project {
    pub fn discover(start: &Path) -> Project {
        let mut dir = Some(start);
        while let Some(d) = dir {
            if d.join(PUBLIC_FILE).is_file() || d.join(STATE_DIR).is_dir() {
                return Project { root: d.to_path_buf() };
            }
            dir = d.parent();
        }
        Project { root: start.to_path_buf() }
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }

    pub fn env_files(&self) -> Result<EnvFiles, Error> {
        EnvFiles::load(&self.root)
    }

    /// A path as the project names it: relative to the root, with `/`.
    pub fn relative(&self, path: &Path) -> String {
        let abs = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
        let rel = abs.strip_prefix(&self.root).map(Path::to_path_buf).unwrap_or(abs);
        rel.components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn load(&self, rel: &str) -> Result<HttpFile, Error> {
        let path = self.root.join(rel);
        let text = std::fs::read_to_string(&path).map_err(|e| Error::Io(rel.to_string(), e))?;
        Ok(parse::parse(&text))
    }

    /// Every `.http` / `.rest` file under the root, sorted; hidden folders, `target` and `node_modules` skipped.
    pub fn http_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.root, &mut |p| {
            if matches!(p.extension().and_then(|e| e.to_str()), Some("http" | "rest")) {
                out.push(self.relative(p));
            }
        });
        out.sort();
        out
    }

    pub fn list(&self) -> Result<Vec<RequestRef>, Error> {
        let mut out = Vec::new();
        for file in self.http_files() {
            for r in self.load(&file)?.requests {
                out.push(RequestRef {
                    file: file.clone(),
                    id: r.id(),
                    name: r.name,
                    index: r.index,
                    method: r.method,
                    url: r.url,
                    title: r.title,
                });
            }
        }
        Ok(out)
    }

    /// Finds the request `id` in `file`, or — no `id` — the file's only request.
    pub fn request(&self, file: &str, id: Option<&str>) -> Result<(HttpFile, Request), Error> {
        let parsed = self.load(file)?;
        let request = match id {
            Some(id) => parsed.find(id).cloned().ok_or_else(|| {
                let ids: Vec<String> = parsed.requests.iter().map(Request::id).collect();
                Error::Usage(format!("{file} 里没有请求 `{id}`，可用的：{}", ids.join(", ")))
            })?,
            None => match parsed.requests.as_slice() {
                [only] => only.clone(),
                [] => return Err(Error::Usage(format!("{file} 里没有请求"))),
                many => {
                    let ids: Vec<String> = many.iter().map(Request::id).collect();
                    return Err(Error::Usage(format!(
                        "{file} 里有 {} 个请求，请用 `{file}#<名字>` 指定一个：{}",
                        many.len(),
                        ids.join(", ")
                    )));
                }
            },
        };
        if let Some(e) = parsed.errors().find(|d| d.line >= request.start_line && d.line < request.end_line) {
            return Err(Error::Parse { file: file.to_string(), line: e.line + 1, message: e.message.clone() });
        }
        Ok((parsed, request))
    }

    /// Finds a request named `name`: in `file` first, then anywhere in the project, where it must be unique.
    pub fn find_named(&self, file: &str, name: &str) -> Result<(String, Request), Error> {
        if let Ok(parsed) = self.load(file) {
            if let Some(r) = parsed.requests.iter().find(|r| r.name.as_deref() == Some(name)) {
                return Ok((file.to_string(), r.clone()));
            }
        }
        let mut found = Vec::new();
        for f in self.http_files() {
            if let Ok(parsed) = self.load(&f) {
                for r in parsed.requests.into_iter().filter(|r| r.name.as_deref() == Some(name)) {
                    found.push((f.clone(), r));
                }
            }
        }
        match found.len() {
            0 => Err(Error::Usage(format!("找不到名为 `{name}` 的请求"))),
            1 => Ok(found.pop().unwrap()),
            _ => Err(Error::Usage(format!(
                "有多个文件里都有名为 `{name}` 的请求：{}",
                found.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        }
    }
}

/// Splits `auth/login.http#login` into the file and the request id.
pub fn split_target(target: &str) -> (&str, Option<&str>) {
    match target.rsplit_once('#') {
        Some((file, id)) if !id.is_empty() => (file, Some(id)),
        Some((file, _)) => (file, None),
        None => (target, None),
    }
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                walk(&path, f);
            }
        } else {
            f(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join(PUBLIC_FILE), "{}").unwrap();
        std::fs::create_dir_all(root.join("auth")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("auth/login.http"), "# @name login\nPOST /login\n\n###\n# @name me\nGET /me\n").unwrap();
        std::fs::write(root.join("health.http"), "GET /ping\n").unwrap();
        std::fs::write(root.join(".git/x.http"), "GET /hidden\n").unwrap();
        let p = Project { root: root.to_path_buf() };
        (dir, p)
    }

    #[test]
    fn discovers_root_from_subfolder() {
        let (_d, p) = project();
        assert_eq!(Project::discover(&p.root.join("auth")).root, p.root);
    }

    #[test]
    fn lists_requests_skipping_hidden_folders() {
        let (_d, p) = project();
        let ids: Vec<String> = p.list().unwrap().iter().map(|r| format!("{}#{}", r.file, r.id)).collect();
        assert_eq!(ids, vec!["auth/login.http#login", "auth/login.http#me", "health.http##1"]);
    }

    #[test]
    fn picks_requests() {
        let (_d, p) = project();
        assert_eq!(p.request("health.http", None).unwrap().1.url, "/ping");
        assert_eq!(p.request("auth/login.http", Some("2")).unwrap().1.id(), "me");
        let err = p.request("auth/login.http", None).unwrap_err().to_string();
        assert!(err.contains("login, me"), "{err}");
        assert_eq!(p.find_named("health.http", "me").unwrap().0, "auth/login.http");
    }

    #[test]
    fn splits_targets() {
        assert_eq!(split_target("a/b.http#login"), ("a/b.http", Some("login")));
        assert_eq!(split_target("a/b.http"), ("a/b.http", None));
    }
}
