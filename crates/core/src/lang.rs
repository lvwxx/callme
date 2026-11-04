//! Language features for editors: the tokens of each line of a `.http` file (the GUI's highlighter maps them onto
//! the editor's colors), fold ranges for requests and for JSON, and the variables `{{…}}` with their ranges.
//! Positions are 0-based lines and byte columns within a line.

use std::ops::Range;

use serde::Serialize;

use crate::parse::METHODS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Token {
    /// `###` separators and comments.
    Comment,
    /// The request's title after `###`, and a request's name.
    Title,
    /// Methods and directives (`@name`, `@no-redirect`).
    Keyword,
    /// Header names, query keys, JSON keys.
    Attr,
    /// `{{variables}}`.
    Variable,
    /// JSON strings, `< ./file` paths.
    String,
    /// JSON numbers, `true`, `false`, `null`.
    Number,
    /// `HTTP/1.1`.
    Version,
}

pub type LineTokens = Vec<(Range<usize>, Token)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Before a block's request line: comments, directives, blank lines.
    Preamble,
    /// After the request line: query continuation lines, then headers.
    Headers,
    Body,
}

/// The section each line of a `.http` text is in (a `###` line is in `Preamble`).
fn sections(text: &str) -> Vec<Section> {
    let mut out = Vec::new();
    let mut section = Section::Preamble;
    for line in text.split('\n') {
        let t = line.trim();
        if t.starts_with("###") {
            section = Section::Preamble;
            out.push(Section::Preamble);
            continue;
        }
        out.push(section);
        match section {
            Section::Preamble if !t.is_empty() && comment(line).is_none() => section = Section::Headers,
            Section::Headers if t.is_empty() => section = Section::Body,
            _ => {}
        }
    }
    out
}

/// The tokens of every line of a `.http` text.
pub fn tokens(text: &str) -> Vec<LineTokens> {
    let mut out = Vec::new();
    let mut section = Section::Preamble;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let mut toks: LineTokens = Vec::new();
        if trimmed.starts_with("###") {
            section = Section::Preamble;
            toks.push((indent..indent + 3, Token::Comment));
            let rest = &line[indent + 3..];
            let title_start = indent + 3 + (rest.len() - rest.trim_start().len());
            if title_start < line.trim_end().len() {
                toks.push((title_start..line.trim_end().len(), Token::Title));
            }
        } else {
            match section {
                Section::Preamble => {
                    if trimmed.is_empty() {
                    } else if let Some(c) = comment(line) {
                        comment_tokens(line, c, &mut toks);
                    } else {
                        request_line(line, &mut toks);
                        section = Section::Headers;
                    }
                }
                Section::Headers => {
                    if trimmed.is_empty() {
                        section = Section::Body;
                    } else if let Some(c) = comment(line) {
                        comment_tokens(line, c, &mut toks);
                    } else if (trimmed.starts_with('?') || trimmed.starts_with('&')) && indent > 0 {
                        query_line(line, indent + 1, &mut toks);
                    } else if let Some(colon) = line.find(':') {
                        toks.push((indent..colon, Token::Attr));
                        variables(line, colon + 1, line.len(), &mut toks);
                    }
                }
                Section::Body => {
                    if let Some(path) = trimmed.strip_prefix("< ") {
                        toks.push((indent..indent + 1, Token::Keyword));
                        toks.push((line.len() - path.len()..line.len(), Token::String));
                    } else {
                        json_line(line, &mut toks);
                    }
                }
            }
        }
        toks.sort_by_key(|(r, _)| r.start);
        out.push(toks);
    }
    out
}

/// The comment's start (`#` or `//`) when `line` is a comment.
fn comment(line: &str) -> Option<usize> {
    let t = line.trim_start();
    (t.starts_with('#') || t.starts_with("//")).then(|| line.len() - t.len())
}

fn comment_tokens(line: &str, start: usize, toks: &mut LineTokens) {
    let body_start = start + if line[start..].starts_with("//") { 2 } else { 1 };
    let body = &line[body_start..];
    let at = body_start + (body.len() - body.trim_start().len());
    if line[at..].starts_with('@') {
        let word_end = line[at..].find(char::is_whitespace).map_or(line.len(), |i| at + i);
        toks.push((start..at, Token::Comment));
        toks.push((at..word_end, Token::Keyword));
        let arg = line[word_end..].trim();
        if !arg.is_empty() {
            let arg_start = line.len() - line[word_end..].trim_start().len();
            toks.push((arg_start..arg_start + arg.len(), Token::Title));
        }
    } else {
        toks.push((start..line.len(), Token::Comment));
    }
}

fn request_line(line: &str, toks: &mut LineTokens) {
    let mut pos = 0;
    let mut first = true;
    for word in line.split(' ') {
        let range = pos..pos + word.len();
        if first && METHODS.contains(&word) {
            toks.push((range, Token::Keyword));
        } else if word.starts_with("HTTP/") {
            toks.push((range, Token::Version));
        } else if !word.is_empty() {
            variables(line, range.start, range.end, toks);
        }
        if !word.is_empty() {
            first = false;
        }
        pos += word.len() + 1;
    }
}

fn query_line(line: &str, key_start: usize, toks: &mut LineTokens) {
    let key_end = line[key_start..].find('=').map_or(line.len(), |i| key_start + i);
    toks.push((key_start..key_end, Token::Attr));
    variables(line, key_end, line.len(), toks);
}

/// `{{…}}` in `line[from..to]`.
fn variables(line: &str, from: usize, to: usize, toks: &mut LineTokens) {
    let mut i = from;
    while let Some(open) = line[i..to].find("{{") {
        let start = i + open;
        match line[start + 2..to].find("}}") {
            Some(close) => {
                let end = start + 2 + close + 2;
                toks.push((start..end, Token::Variable));
                i = end;
            }
            None => break,
        }
    }
}

/// JSON tokens of one line: strings (keys apart), numbers, literals, variables. Lines that are not JSON get
/// variables only, from the strings scan.
fn json_line(line: &str, toks: &mut LineTokens) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                let end = (i + 1).min(bytes.len());
                let is_key = line[end..].trim_start().starts_with(':');
                // Variables inside a string are tokens of their own.
                let mut vars = Vec::new();
                variables(line, start, end, &mut vars);
                let mut at = start;
                for (r, t) in vars {
                    if at < r.start {
                        toks.push((at..r.start, if is_key { Token::Attr } else { Token::String }));
                    }
                    at = r.end;
                    toks.push((r, t));
                }
                if at < end {
                    toks.push((at..end, if is_key { Token::Attr } else { Token::String }));
                }
                i = end;
            }
            b'{' if bytes.get(i + 1) == Some(&b'{') => match line[i + 2..].find("}}") {
                Some(close) => {
                    let end = i + 2 + close + 2;
                    toks.push((i..end, Token::Variable));
                    i = end;
                }
                None => i += 2,
            },
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len() && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    i += 1;
                }
                toks.push((start..i, Token::Number));
            }
            b't' | b'f' | b'n' => {
                let word = ["true", "false", "null"].into_iter().find(|w| line[i..].starts_with(w));
                match word {
                    Some(w) if !line[i + w.len()..].starts_with(|c: char| c.is_alphanumeric()) => {
                        toks.push((i..i + w.len(), Token::Number));
                        i += w.len();
                    }
                    _ => i += 1,
                }
            }
            _ => i += 1,
        }
    }
}

/// A fold: line `start` stays, lines after it through `end` fold away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fold {
    pub start_line: usize,
    pub end_line: usize,
}

/// One fold per request of a `.http` text: from its first line to its last line that is not blank.
pub fn http_folds(text: &str) -> Vec<Fold> {
    let f = crate::parse::parse(text);
    let lines: Vec<&str> = text.lines().collect();
    f.requests
        .iter()
        .filter_map(|r| {
            // A block starts after its `###` line, which the fold starts on.
            let separator = r.start_line.checked_sub(1).filter(|&l| lines[l].trim_start().starts_with("###"));
            let first = separator.or_else(|| (r.start_line..r.end_line).find(|&l| !lines[l].trim().is_empty()))?;
            let last = (r.start_line..r.end_line).rev().find(|&l| !lines[l].trim().is_empty())?;
            (last > first).then_some(Fold { start_line: first, end_line: last })
        })
        .collect()
}

/// One fold per object or array spanning lines of a JSON text; the closing bracket's line stays visible.
pub fn json_folds(text: &str) -> Vec<Fold> {
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'{' | b'[' => stack.push(n),
                b'}' | b']' => {
                    if let Some(open) = stack.pop() {
                        if n > open + 1 {
                            out.push(Fold { start_line: open, end_line: n - 1 });
                        }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    out.sort_by_key(|f| f.start_line);
    out
}

/// The UTF-16 column of byte column `byte` of `line` (editors count UTF-16 units).
pub fn utf16_col(line: &str, byte: usize) -> usize {
    line[..byte.min(line.len())].encode_utf16().count()
}

/// The byte column of UTF-16 column `col` of `line`.
pub fn byte_col(line: &str, col: usize) -> usize {
    let mut units = 0;
    for (i, c) in line.char_indices() {
        if units >= col {
            return i;
        }
        units += c.len_utf16();
    }
    line.len()
}

/// The 1-based index of the request whose block holds `line`; the nearest one before it on a blank line between
/// requests.
pub fn request_at(text: &str, line: usize) -> Option<usize> {
    let f = crate::parse::parse(text);
    f.requests
        .iter()
        .rev()
        .find(|r| r.start_line <= line || r.start_line == line + 1)
        .or_else(|| f.requests.first())
        .map(|r| r.index)
}

/// What the caret is in, for completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CompletionContext {
    /// Inside `{{…`, `prefix` typed after `{{`.
    Variable { prefix: String },
    /// A header's name.
    HeaderName,
    /// A header's value, after `name:`.
    HeaderValue { name: String },
    /// The method of a request line.
    Method,
    /// After `# @`.
    Directive,
    None,
}

/// The completion context at byte column `col` of line `line`.
pub fn completion_context(text: &str, line: usize, col: usize) -> CompletionContext {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(src) = lines.get(line) else { return CompletionContext::None };
    let before = &src[..col.min(src.len())];
    if let Some(open) = before.rfind("{{") {
        if !before[open..].contains("}}") {
            return CompletionContext::Variable { prefix: before[open + 2..].trim_start().to_string() };
        }
    }
    let section = sections(text).get(line).copied().unwrap_or(Section::Preamble);
    let trimmed = before.trim_start();
    if comment(src).is_some() {
        let body = trimmed.trim_start_matches('#').trim_start_matches("//").trim_start();
        return if body.starts_with('@') && !body.contains(char::is_whitespace) {
            CompletionContext::Directive
        } else {
            CompletionContext::None
        };
    }
    match section {
        Section::Preamble if !trimmed.contains(char::is_whitespace) && !trimmed.starts_with("###") => CompletionContext::Method,
        Section::Headers if trimmed.starts_with('?') || trimmed.starts_with('&') => CompletionContext::None,
        Section::Headers => match before.split_once(':') {
            None if !trimmed.contains(char::is_whitespace) => CompletionContext::HeaderName,
            Some((name, _)) => CompletionContext::HeaderValue { name: name.trim().to_string() },
            None => CompletionContext::None,
        },
        _ => CompletionContext::None,
    }
}

/// The `{{…}}` at byte column `col` of `line`: its expression and byte range.
pub fn variable_at(line: &str, col: usize) -> Option<(String, Range<usize>)> {
    let mut toks = Vec::new();
    variables(line, 0, line.len(), &mut toks);
    toks.into_iter()
        .map(|(r, _)| r)
        .find(|r| r.start <= col && col <= r.end)
        .map(|r| (line[r.start + 2..r.end - 2].trim().to_string(), r))
}

/// Every `{{…}}` of a text: line, byte range, expression.
pub fn all_variables(text: &str) -> Vec<(usize, Range<usize>, String)> {
    let mut out = Vec::new();
    for (n, line) in text.split('\n').enumerate() {
        let mut toks = Vec::new();
        variables(line, 0, line.len(), &mut toks);
        for (r, _) in toks {
            let expr = line[r.start + 2..r.end - 2].trim().to_string();
            out.push((n, r, expr));
        }
    }
    out
}

/// Common request headers and values offered for them.
pub const HEADERS: &[(&str, &[&str])] = &[
    ("Accept", &["application/json", "*/*", "text/event-stream", "text/html", "application/xml"]),
    ("Accept-Encoding", &["gzip, deflate, br", "identity"]),
    ("Accept-Language", &["zh-CN,zh;q=0.9,en;q=0.8", "en-US,en;q=0.9"]),
    ("Authorization", &["Bearer ", "Basic "]),
    ("Cache-Control", &["no-cache", "no-store", "max-age=0"]),
    ("Connection", &["keep-alive", "close"]),
    ("Content-Type", &["application/json", "application/x-www-form-urlencoded", "multipart/form-data", "text/plain", "application/xml"]),
    ("Cookie", &[]),
    ("If-None-Match", &[]),
    ("If-Modified-Since", &[]),
    ("Origin", &[]),
    ("Referer", &[]),
    ("User-Agent", &[]),
    ("X-Request-Id", &["{{$uuid}}"]),
    ("X-Forwarded-For", &[]),
];

pub const DIRECTIVES: &[(&str, &str)] = &[
    ("name", "给请求命名"),
    ("no-redirect", "不跟随重定向"),
    ("no-cookie-jar", "不读写 cookie"),
    ("timeout", "超时秒数"),
    ("insecure", "忽略 TLS 证书错误"),
];

pub const DYNAMIC: &[(&str, &str)] = &[
    ("$uuid", "随机 UUID"),
    ("$timestamp", "Unix 时间戳（秒）"),
    ("$isoTimestamp", "ISO 8601 时间"),
    ("$randomInt", "0–999 的随机整数"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns() {
        assert_eq!(utf16_col("你好a", "你好".len()), 2);
        assert_eq!(byte_col("你好a", 2), "你好".len());
        assert_eq!(byte_col("ab", 9), 2);
    }

    #[test]
    fn finds_the_request_at_a_line() {
        let t = "### a\nGET /a\n\n### b\nGET /b\n";
        assert_eq!(request_at(t, 1), Some(1));
        assert_eq!(request_at(t, 2), Some(1));
        assert_eq!(request_at(t, 3), Some(2));
        assert_eq!(request_at(t, 4), Some(2));
        assert_eq!(request_at("# @name x\nGET /x", 0), Some(1));
    }

    #[test]
    fn completion_contexts() {
        let t = "# @na\nPO\nAcc\nContent-Type: app\nX: {{base\n\n{\"a\": \"{{login.res\"}";
        let at = |line: usize, col: usize| completion_context(t, line, col);
        assert_eq!(at(0, 5), CompletionContext::Directive);
        assert_eq!(at(1, 2), CompletionContext::Method);
        assert_eq!(at(2, 3), CompletionContext::HeaderName);
        assert_eq!(at(3, 17), CompletionContext::HeaderValue { name: "Content-Type".into() });
        assert_eq!(at(4, 9), CompletionContext::Variable { prefix: "base".into() });
        assert_eq!(at(6, 18), CompletionContext::Variable { prefix: "login.res".into() });
        assert_eq!(at(6, 2), CompletionContext::None);
    }

    #[test]
    fn variables_at_a_column() {
        assert_eq!(variable_at("GET {{ baseUrl }}/a", 8), Some(("baseUrl".into(), 4..17)));
        assert_eq!(variable_at("GET {{baseUrl}}/a", 1), None);
        assert_eq!(all_variables("a\n{{x}} {{y.z}}").len(), 2);
    }

    fn kinds(text: &str, line: usize) -> Vec<(String, Token)> {
        let lines: Vec<&str> = text.split('\n').collect();
        tokens(text)[line].iter().map(|(r, t)| (lines[line][r.clone()].to_string(), *t)).collect()
    }

    const SAMPLE: &str = "### 登录
# @name login
POST {{baseUrl}}/auth/login HTTP/1.1
    ?a={{x}}
Content-Type: application/json
# X-Off: 1

{\"user\": \"{{user}}\", \"n\": -1.5, \"ok\": true}
< ./body.json";

    #[test]
    fn tokenizes_each_section() {
        assert_eq!(kinds(SAMPLE, 0), vec![("###".into(), Token::Comment), ("登录".into(), Token::Title)]);
        assert_eq!(
            kinds(SAMPLE, 1),
            vec![("# ".into(), Token::Comment), ("@name".into(), Token::Keyword), ("login".into(), Token::Title)]
        );
        assert_eq!(
            kinds(SAMPLE, 2),
            vec![("POST".into(), Token::Keyword), ("{{baseUrl}}".into(), Token::Variable), ("HTTP/1.1".into(), Token::Version)]
        );
        assert_eq!(kinds(SAMPLE, 3), vec![("a".into(), Token::Attr), ("{{x}}".into(), Token::Variable)]);
        assert_eq!(kinds(SAMPLE, 4), vec![("Content-Type".into(), Token::Attr)]);
        assert_eq!(kinds(SAMPLE, 5), vec![("# X-Off: 1".into(), Token::Comment)]);
        assert_eq!(
            kinds(SAMPLE, 7),
            vec![
                ("\"user\"".into(), Token::Attr),
                ("\"".into(), Token::String),
                ("{{user}}".into(), Token::Variable),
                ("\"".into(), Token::String),
                ("\"n\"".into(), Token::Attr),
                ("-1.5".into(), Token::Number),
                ("\"ok\"".into(), Token::Attr),
                ("true".into(), Token::Number),
            ]
        );
        assert_eq!(kinds(SAMPLE, 8), vec![("<".into(), Token::Keyword), ("./body.json".into(), Token::String)]);
    }

    #[test]
    fn bare_variables_in_a_body() {
        assert_eq!(kinds("POST /a\n\n{\"id\": {{id}}}", 2), vec![("\"id\"".into(), Token::Attr), ("{{id}}".into(), Token::Variable)]);
    }

    #[test]
    fn folds_requests_and_json() {
        assert_eq!(http_folds("### a\nGET /a\nX: 1\n\n### b\nGET /b\n"), vec![Fold { start_line: 0, end_line: 2 }, Fold { start_line: 4, end_line: 5 }]);
        let json = "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"s\": \"{[\",\n  \"o\": {}\n}";
        assert_eq!(json_folds(json), vec![Fold { start_line: 0, end_line: 6 }, Fold { start_line: 1, end_line: 3 }]);
    }
}
