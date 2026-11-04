//! `callme`: run `.http` requests from the terminal, from scripts and from AI agents. Output is for people in a
//! terminal and JSON otherwise (`--format` forces either). Exit codes: 0 a response arrived (4xx / 5xx included,
//! unless `--fail`), 1 the request could not be made, 2 the input is wrong.

mod human;
mod mcp;

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use callme_core::history;
use callme_core::import;
use callme_core::output::{self, DEFAULT_MAX_BODY};
use callme_core::project::{split_target, Project};
use callme_core::run::{self, RunOptions};
use callme_core::Error;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};

pub const FORMAT_GUIDE: &str = include_str!("format.md");

#[derive(Parser)]
#[command(name = "callme", version, about = "简洁、AI 友好的 API 调试工具：运行 .http 文件里的请求", disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// 输出格式；默认在终端里给人看，否则输出 JSON
    #[arg(long, global = true, value_enum)]
    format: Option<Format>,
    /// 项目目录（默认：从当前目录往上找）
    #[arg(short = 'C', long = "dir", global = true)]
    dir: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Human,
    Json,
}

#[derive(Subcommand)]
enum Cmd {
    /// 执行请求：callme run auth/login.http#login
    Run {
        /// <文件>[#名字或序号]；文件里只有一个请求时可以不写 #
        target: String,
        /// 环境（默认：上次选的）
        #[arg(short, long)]
        env: Option<String>,
        /// 临时覆盖变量：--var userId=42
        #[arg(long = "var", value_name = "NAME=VALUE")]
        vars: Vec<String>,
        /// 4xx、5xx 时退出码为 1
        #[arg(long)]
        fail: bool,
        /// JSON 输出里响应体的最大字节数，超出就截断（完整内容在历史记录里）
        #[arg(long, default_value_t = DEFAULT_MAX_BODY)]
        max_body: usize,
        /// 同时显示响应头（给人看的格式）
        #[arg(short = 'i', long)]
        include: bool,
    },
    /// 列出项目里所有请求
    Ls,
    /// 列出环境和变量（密钥打码）；--use 切换当前环境
    Env {
        /// 要查看的环境（默认：当前环境）
        name: Option<String>,
        /// 把它设为当前环境
        #[arg(long = "use")]
        use_it: bool,
    },
    /// 查看请求最近几次的响应
    History {
        /// <文件>[#名字或序号]
        target: String,
        #[arg(short = 'n', default_value_t = 5)]
        limit: usize,
        #[arg(long, default_value_t = DEFAULT_MAX_BODY)]
        max_body: usize,
    },
    /// 把标准输入里的 curl 命令或 fetch(…) 转成 .http
    Import {
        /// 追加到这个文件（默认输出到标准输出）
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 请求名（# @name）
        #[arg(long)]
        name: Option<String>,
        /// 用这个环境的变量替换 URL 前缀和密钥（默认：当前环境）
        #[arg(short, long)]
        env: Option<String>,
    },
    /// 帮助：callme help format 查看 .http 语法
    Help { topic: Option<String> },
    /// 以 stdio 方式启动 MCP server
    Mcp,
    /// 在 callme 桌面版里打开项目（默认当前目录）
    Open {
        /// 项目目录
        dir: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = match cli.format {
        Some(f) => f == Format::Json,
        None => !std::io::stdout().is_terminal(),
    };
    let start = cli.dir.clone().unwrap_or_else(|| std::env::current_dir().expect("a working directory"));
    let project = Project::discover(&start);
    match dispatch(cli.cmd, &project, json) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&e.to_json()).unwrap());
            } else {
                human::error(&e);
            }
            ExitCode::from(e.exit_code() as u8)
        }
    }
}

fn dispatch(cmd: Cmd, project: &Project, json: bool) -> Result<u8, Error> {
    match cmd {
        Cmd::Run { target, env, vars, fail, max_body, include } => {
            let (file, id) = split_target(&target);
            let file = project.relative(std::path::Path::new(file));
            let overrides = parse_vars(&vars)?;
            if json {
                let result = run::run(project, &file, id, RunOptions { env, overrides, on_chunk: None })?;
                print_json(&output::record_json(&result.record, Some(&result.raw_body), Some(&result.history_file), max_body));
                return Ok(if fail && result.record.response.status >= 400 { 1 } else { 0 });
            }
            let mut live = human::Live::new(include);
            let result = run::run(project, &file, id, RunOptions { env, overrides, on_chunk: Some(&mut live) })?;
            human::finish(&result, &mut live);
            Ok(if fail && result.record.response.status >= 400 { 1 } else { 0 })
        }
        Cmd::Ls => {
            let list = project.list()?;
            if json {
                print_json(&serde_json::to_value(&list).unwrap());
            } else {
                human::list(&list);
            }
            Ok(0)
        }
        Cmd::Env { name, use_it } => {
            let files = project.env_files()?;
            if use_it {
                let name = name.as_deref().ok_or_else(|| Error::Usage("用法：callme env <环境> --use".into()))?;
                files.resolve(Some(name), &[])?;
                run::set_current_env(project, name)?;
            }
            let current = run::current_env(project);
            let shown = name.or_else(|| current.clone());
            let vars = files.resolve(shown.as_deref(), &[])?;
            if json {
                let vars_json: serde_json::Map<String, Value> = vars
                    .vars
                    .iter()
                    .map(|(k, v)| {
                        let value = if v.source == callme_core::env::Source::Private { "***".to_string() } else { v.value.clone() };
                        (k.clone(), json!({ "value": value, "source": v.source }))
                    })
                    .collect();
                print_json(&json!({ "envs": files.names(), "current": current, "env": shown, "vars": vars_json }));
            } else {
                human::env(&files.names(), current.as_deref(), &vars);
            }
            Ok(0)
        }
        Cmd::History { target, limit, max_body } => {
            let (file, id) = split_target(&target);
            let file = project.relative(std::path::Path::new(file));
            let (_, request) = project.request(&file, id)?;
            let records = history::list(project, &file, &request.id(), limit);
            if json {
                let items: Vec<Value> = records.iter().map(|(p, r)| output::record_json(r, None, Some(p), max_body)).collect();
                print_json(&Value::Array(items));
            } else {
                human::history(&records);
            }
            Ok(0)
        }
        Cmd::Import { output, name, env } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).map_err(|e| Error::Io("标准输入".into(), e))?;
            let imported = import::parse_any(&input)?;
            let env = env.or_else(|| run::current_env(project));
            let vars = project.env_files()?.resolve(env.as_deref(), &[]).ok();
            let converted = import::to_http(&imported, vars.as_ref(), name.as_deref());
            if let Some(path) = &output {
                let existing = std::fs::read_to_string(path).unwrap_or_default();
                let mut text = existing.clone();
                if !text.trim().is_empty() {
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                    text.push_str("\n###\n");
                }
                text.push_str(&converted.text);
                std::fs::write(path, text).map_err(|e| Error::Io(path.display().to_string(), e))?;
            }
            if json {
                print_json(&json!({ "text": converted.text, "replaced": converted.replaced, "file": output }));
            } else {
                match &output {
                    None => print!("{}", converted.text),
                    Some(path) => eprintln!("已写入 {}", path.display()),
                }
                if !converted.replaced.is_empty() {
                    eprintln!("已替换为变量：{}", converted.replaced.join(", "));
                }
            }
            Ok(0)
        }
        Cmd::Help { topic } => {
            match topic.as_deref() {
                Some("format") => print!("{FORMAT_GUIDE}"),
                _ => {
                    use clap::CommandFactory;
                    Cli::command().print_long_help().ok();
                    println!("\n更多：callme help format  查看 .http 文件语法");
                }
            }
            Ok(0)
        }
        Cmd::Open { dir } => {
            let start = match dir {
                Some(d) => std::fs::canonicalize(&d).map_err(|e| Error::Io(d.display().to_string(), e))?,
                None => project.root.clone(),
            };
            let root = Project::discover(&start).root;
            let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| Error::Usage("没有 HOME 环境变量".into()))?;
            let file = home.join(".callme").join("last-project");
            std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| Error::Io(file.display().to_string(), e))?;
            std::fs::write(&file, root.display().to_string()).map_err(|e| Error::Io(file.display().to_string(), e))?;
            // By bundle id, else by name (an app LaunchServices has not registered yet).
            let launch = |args: &[&str]| {
                std::process::Command::new("open").args(args).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
            };
            match launch(&["-b", "dev.callme.app"]) || launch(&["-a", "callme"]) {
                true => {
                    if !json {
                        eprintln!("已在 callme 里打开 {}", root.display());
                    }
                    Ok(0)
                }
                false => Err(Error::Usage(format!(
                    "没有找到 callme 桌面版（把 callme.app 放进「应用程序」文件夹）。已记下项目 {}，打开 app 就是它",
                    root.display()
                ))),
            }
        }
        Cmd::Mcp => {
            mcp::serve(project);
            Ok(0)
        }
    }
}

pub fn parse_vars(vars: &[String]) -> Result<Vec<(String, String)>, Error> {
    vars.iter()
        .map(|v| {
            v.split_once('=')
                .map(|(k, v)| (k.trim().to_string(), v.to_string()))
                .ok_or_else(|| Error::Usage(format!("--var 应写成 NAME=VALUE，收到 `{v}`")))
        })
        .collect()
}

fn print_json(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", serde_json::to_string_pretty(v).unwrap());
}
