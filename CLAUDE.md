# callme

一个简洁、AI 友好的 API 调试客户端，用来替代 Postman。

## 背景

- 出发点：Postman 太臃肿，想要一个轻量的、AI 可以直接读写和驱动的 API 调试工具。
- 项目名定为 **callme**（2026-11-04）。名字的意思是：请求文件本身就是文档，人和 AI 都能直接拿来「call」。

## 已有的设计方向（尚未确认）

- **请求存成纯文本文件**：可 diff、可进 git，AI 能直接读写（文件扩展名暂定 `.call`，还没定）。
- **CLI 优先**：命令短，写在 AI 的 prompt 或 tool 定义里都自然。
- **可接入 MCP**：让 AI agent 能直接调用 callme 发请求。
- 定位轻量，不做 Postman 那种大而全。

## 技术栈

- 使用 **nsh (Native Shell)** 开发：https://github.com/longbridge/nsh ，文档 https://nsh.dev （给模型读的版本：https://nsh.dev/llms.txt）。
  - Longbridge 自研的原生 UI 框架，Rust 内核；界面可用 TSX（QuickJS 运行、支持热更新）或 Rust 写，也可以混合。
  - 桌面端用原生控件渲染（macOS AppKit / Windows Win32 / Linux GTK 4），也能打包成 Web 静态站点。
  - 和 API 客户端相关的能力：
    - `fetch` 走系统网络栈；可以设置 `Cookie`、`User-Agent`、`Origin` 等浏览器禁止设置的请求头；支持 `redirect: "manual"`，能拿到 3xx 原始响应；支持流式请求体和响应体。
    - `nsh/base` 自带 `Editor`（代码编辑器）、`DataTable`、`Tabs`、`Tree`、`Command`（命令面板）、`ResizablePanelGroup`、`DockArea`、`DiffView`。
    - `nsh/desktop` 提供菜单和标题栏。
    - 有 `fs/promises`、`process` 等标准模块。
  - 能访问哪些域名、文件、命令要在 `nsh.json` 的 `capabilities` 里声明。调试工具要能访问任意地址，需要配置 `network.hosts: ["*"]`。

## 设计

完整设计见 `docs/superpowers/specs/2026-11-04-callme-design.md`；界面设计稿：https://claude.ai/artifact/Jnu3TcGtoqcaLqKixQnwsn

已确定的要点：
- 定位：AI 从外部驱动 callme（通过文件、CLI、MCP），不内置 AI 助手。
- 第一版只做 HTTP/REST，加上 SSE 流式响应。
- 架构：Rust core（`crates/core`）+ CLI（`crates/cli`，包括 MCP）+ nsh 混合模式 GUI（`app/`，用 TSX 写界面）。
- 请求文件用 `.http`，取 JetBrains 和 REST Client 兼容的那部分语法，不支持 JS 脚本；环境文件沿用 JetBrains 格式。
- 编辑请求和环境都是写文本，不做表格。
- 视觉：Longbridge GPUI 设计系统，主题色 `#00B8B8`。

## 待决定

- 包名、仓库名、域名是否可用（还没查）

## 开发

```sh
cargo test --workspace          # 单元测试 + 本地 HTTP 服务的集成测试
cargo clippy --workspace --all-targets
cargo run -p callme -- help format
```

- `crates/core`：parse（.http 解析）、env（环境合并、密钥打码）、resolve（变量、响应引用、动态值）、exec（libcurl 执行、分阶段计时）、history、import（curl/fetch → .http）、output（CLI/MCP 共用的 JSON）、run（串起整个流程）。
- `crates/cli`：`callme` 命令和 `callme mcp`；`src/format.md` 是给 AI 看的语法说明，也是 MCP 的 instructions。
- HTTP 执行用 libcurl 而不是 reqwest：要拿 DNS/连接/TLS/首字节各阶段耗时。
- `app/`：nsh 混合模式的 GUI（TSX 界面 + Rust crate）。`src/lib.rs` 注册 `.http` 语言和高亮器（用 core 的 `lang::tokens`），`Callme` 模块在 `blocking` 线程里跑请求，SSE 事件经 channel 回到引擎线程再 `post` 给 TSX。
- nsh 要用源码版（`~/.nsh/source`，0.4.0）：crates.io 的 0.2.0 没有 `Editor`。`app/Cargo.toml` 目前依赖本机绝对路径，后面要换成 git 依赖。
- GUI 测试：`scripts/test-app.sh`（无界面）；加 `tests/screenshot.test.tsx --target macos` 在真实窗口里截图。
- GUI 结构：`api.ts`（native 调用）、`store.ts`（状态、自动保存、外部修改检测、发送）、`TitleBar` / `Sidebar` / `RequestPane` / `ResponsePane` / `EnvPane` / `Palette`；主题在 `theme.json`（Longbridge GPUI 色值）。
- nsh 的坑：无障碍属性是 `accessibilityLabel` / `accessibilityState`（不是 aria-*）；自定义主题色不能加 `/60` 透明度；无界面测试的 `texts()` 不含补全和悬停浮层，要在 `--target macos` 下截图看。
- 进度：M0、M1（core + CLI）、M2（MCP）、M3（GUI 主体）已完成。
