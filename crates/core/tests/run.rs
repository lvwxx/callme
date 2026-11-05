//! Runs requests end to end against a small local HTTP/1.1 server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

use callme_core::history::BodyKind;
use callme_core::project::Project;
use callme_core::run::{run, RunOptions};
use callme_core::Error;

fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || handle(stream));
        }
    });
    format!("http://{addr}")
}

fn handle(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut headers = Vec::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end().to_string();
        if line.is_empty() {
            break;
        }
        if let Some((n, v)) = line.split_once(':') {
            if n.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap();
            }
            headers.push((n.to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap().to_string();
    let path = parts.next().unwrap().to_string();
    let header = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()).unwrap_or_default();

    let (status, extra, ctype, out): (&str, String, &str, String) = match path.as_str() {
        "/echo" => {
            let echo = serde_json::json!({
                "method": method,
                "contentType": header("content-type"),
                "auth": header("authorization"),
                "ua": header("user-agent"),
                "body": String::from_utf8_lossy(&body),
            });
            ("200 OK", String::new(), "application/json", echo.to_string())
        }
        "/login" => ("200 OK", "Set-Cookie: session=abc; Path=/\r\nX-Trace: t-1\r\n".into(), "application/json", r#"{"token":"tok-42","user":{"id":7}}"#.into()),
        "/whoami" => ("200 OK", String::new(), "text/plain", header("cookie")),
        "/redirect" => ("302 Found", "Location: /echo\r\n".into(), "text/plain", "moved".into()),
        "/slow" => {
            thread::sleep(Duration::from_millis(1500));
            ("200 OK", String::new(), "text/plain", "late".into())
        }
        "/sse" => ("200 OK", String::new(), "text/event-stream", "event: delta\ndata: {\"t\":\"hi\"}\n\ndata: done\n\n".into()),
        _ => ("404 Not Found", String::new(), "text/plain", "no".into()),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn project(base: &str) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let w = |p: &str, t: &str| std::fs::write(root.join(p), t).unwrap();
    w("http-client.env.json", &format!(r#"{{"dev": {{"baseUrl": "{base}", "user": "ada"}}}}"#));
    w("http-client.private.env.json", r#"{"dev": {"password": "pa55word-secret"}}"#);
    w(
        "api.http",
        "### 登录
# @name login
POST {{baseUrl}}/login

### 回显
# @name echo
POST {{baseUrl}}/echo
Authorization: Bearer {{login.response.body.$.token}}

{\"user\": \"{{user}}\", \"password\": \"{{password}}\"}

###
# @name whoami
GET {{baseUrl}}/whoami

###
# @name nocookie
# @no-cookie-jar
GET {{baseUrl}}/whoami

###
# @name redirect
GET {{baseUrl}}/redirect

###
# @name stay
# @no-redirect
GET {{baseUrl}}/redirect

###
# @name slow
# @timeout 0.3
GET {{baseUrl}}/slow

###
# @name sse
GET {{baseUrl}}/sse

###
# @name missing
GET {{baseUrl}}/nope
",
    );
    (dir, Project { root })
}

fn go(p: &Project, name: &str) -> Result<callme_core::run::RunResult, Error> {
    run(p, "api.http", Some(name), RunOptions { env: Some("dev".into()), ..Default::default() })
}

#[test]
fn runs_requests_end_to_end() {
    let base = serve();
    let (_d, p) = project(&base);

    // A reference to a request that has not run yet.
    let err = go(&p, "echo").err().unwrap();
    assert!(err.to_string().contains("还没有运行过"), "{err}");

    let login = go(&p, "login").unwrap();
    assert_eq!(login.record.response.status, 200);
    assert_eq!(login.record.response.body_kind, BodyKind::Json);
    assert!(login.record.timing.total > 0.0);
    assert!(Path::new(&p.root.join(&login.history_file)).is_file());

    let echo = go(&p, "echo").unwrap();
    let body = &echo.record.response.body;
    assert_eq!(body["method"], "POST");
    assert_eq!(body["auth"], "Bearer tok-42", "response reference");
    assert_eq!(body["contentType"], "application/json", "added for a JSON body");
    let ua = body["ua"].as_str().unwrap();
    assert!(ua.starts_with("curl/") && ua.contains(" callme/"), "{ua}");
    assert_eq!(body["body"], r#"{"user": "ada", "password": "pa55word-secret"}"#);
    assert_eq!(echo.record.request.added_headers, vec!["Content-Type"]);

    // The cookie login set is sent back, unless the request opts out.
    assert_eq!(go(&p, "whoami").unwrap().record.response.body, "session=abc");
    assert_eq!(go(&p, "nocookie").unwrap().record.response.body_kind, BodyKind::Empty);

    let redirected = go(&p, "redirect").unwrap();
    assert_eq!(redirected.record.response.status, 200);
    assert_eq!(redirected.record.response.body["method"], "GET");
    assert_eq!(go(&p, "stay").unwrap().record.response.status, 302);

    let sse = go(&p, "sse").unwrap();
    assert_eq!(sse.record.response.body_kind, BodyKind::Sse);
    assert_eq!(sse.record.response.body[0]["data"]["t"], "hi");
    assert_eq!(sse.record.response.body[1]["data"], "done");

    assert_eq!(go(&p, "missing").unwrap().record.response.status, 404);

    let slow = go(&p, "slow").err().unwrap();
    assert_eq!(slow.kind(), "timeout");
    assert_eq!(slow.exit_code(), 1);
}

#[test]
fn secrets_are_masked_in_history() {
    let base = serve();
    let (_d, p) = project(&base);
    go(&p, "login").unwrap();
    let echo = go(&p, "echo").unwrap();
    let saved = std::fs::read_to_string(p.root.join(&echo.history_file)).unwrap();
    let request = &echo.record.request;
    assert!(!format!("{request:?}").contains("pa55word-secret"));
    // The response is stored as received.
    assert!(saved.contains("pa55word-secret"));
}

#[test]
fn connection_refused_is_a_network_error() {
    let (_d, p) = project("http://127.0.0.1:9");
    let err = go(&p, "login").err().unwrap();
    assert_eq!(err.kind(), "network");
}
