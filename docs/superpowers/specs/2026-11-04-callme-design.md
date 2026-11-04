# callme 设计文档

- 日期：2026-11-04
- 状态：设计已确认；M1（core + CLI）和 M2（MCP）已实现
- 界面设计稿：https://claude.ai/artifact/Jnu3TcGtoqcaLqKixQnwsn

## 1. 目标

做一个替代 Postman 的 API 调试客户端，满足五个特性：

| 特性 | 落地方式 |
|---|---|
| 简洁 | 没有账号、云同步、团队空间；一个文件夹就是一个项目；单窗口三栏 |
| 易用 | 键盘优先（`⌘K`、`⌘Enter`）；粘贴 curl 自动转换；直接写文本，不用表单 |
| 速度快 | nsh 原生渲染，不用 Electron；请求就是文件，不用数据库；大响应虚拟滚动 |
| 美观现代 | Longbridge GPUI 设计系统，深色和浅色两套主题 |
| AI 友好 | 请求存成纯文本 `.http` 文件；提供 CLI 和 MCP；GUI 和 AI 操作同一批文件 |

### 定位

**AI 从外部驱动 callme**：Claude Code 这类 agent 在代码仓库里直接读写 `.http` 文件，通过 CLI 或 MCP 发请求、读响应；GUI 给人看和手动调试。callme **不内置** AI 助手（不接模型、不管 API Key、不做对话界面）。

### 第一版不做的

- gRPC、WebSocket（第一版只做 HTTP/REST，加上 SSE 流式响应）
- JetBrains `.http` 的 JS 脚本（`> {% ... %}`、`< {% ... %}`）
- 表格方式编辑请求头和参数
- Mock Server、文档发布、监控、团队协作、云同步
- 内置 AI 助手

## 2. 架构

一个 Rust core 加三个入口，三个入口共用同一套解析和执行逻辑，所以同一个文件在 GUI 里发送和让 AI 用 CLI 跑，结果完全一致。

```
callme/
├── crates/core      不依赖任何界面
│   ├── parse        .http 解析（请求、指令、变量、引用）→ 带位置信息的语法树
│   ├── env          读取环境文件，按优先级合并变量
│   ├── resolve      变量替换、响应引用求值（JSONPath）
│   ├── exec         用 libcurl 发请求：分阶段计时、SSE、cookie、重定向、超时
│   ├── history      读写 .callme/history
│   ├── import       curl、DevTools「Copy as fetch」→ .http，并自动换成变量
│   ├── jsonpath     响应过滤、响应引用共用
│   └── lang         语言服务：高亮、补全、诊断、跳转、折叠范围、悬停信息
├── crates/cli       callme 命令行，包括 `callme mcp`（MCP server）
└── app/             nsh 混合模式项目（nsh init --template mixed）
    ├── src/         Rust：把 core 暴露成 nsh 模块；注册 .http 语言和高亮器
    ├── *.tsx        界面：Sidebar、RequestEditor、ResponsePanel、EnvView、CommandPalette
    └── nsh.json     能力声明：fs 只能读写项目目录
```

要点：

- **GUI 不用 nsh 的 `fetch`**，请求由 core 通过 libcurl（Rust 的 `curl` crate）发出。libcurl 原生提供 DNS、连接、TLS、首字节、下载各阶段的耗时，证书忽略、重定向、cookie jar、HTTP/2 和解压也都自带，而且和 CLI 的行为一致。（最初设想用 reqwest，但 reqwest 拿不到分阶段耗时。）
- **`lang` 放在 core 里**：GUI 里的高亮器用 Rust 写，通过 nsh 的 `register_language` 和 `register_highlighter` 注册。补全、诊断、跳转由 TSX 调用 core 拿结果，再交给 `Editor`。以后要给 VS Code、Zed 做 LSP，也只是在 `lang` 外面再包一层。
- **GUI 和 CLI 只是很薄的外壳**，业务逻辑都在 core 里，这样 core 可以单独测试。

## 3. 文件格式与项目结构

### 3.1 请求文件 `.http`

采用 JetBrains 和 VS Code REST Client 都兼容的那部分语法：

```http
### 登录
# @name login
POST {{baseUrl}}/auth/login
Content-Type: application/json

{"user": "{{user}}", "password": "{{password}}"}

### 获取当前用户
# @name me
GET {{baseUrl}}/me
Authorization: Bearer {{login.response.body.$.token}}
```

- 一个文件可以放多个请求，用 `###` 分隔，`###` 后面的文字是请求标题。
- `# @name <name>` 给请求命名，在文件内唯一；没有名字的请求用「文件路径 + 序号」指代，比如 `auth/login.http#2`。
- 结构：请求行（方法、URL，可以接 `HTTP/1.1` 这类版本）→ 请求头 → 空行 → 请求体。URL 可以跨多行写 query（以 `?` 和 `&` 开头的续行）。
- 变量：`{{name}}` 取环境变量。
- 响应引用：`{{<请求名>.response.body.<JSONPath>}}`、`{{<请求名>.response.headers.<Header>}}`，取的是这个请求**最近一次的响应**（从历史记录里读）。
- 注释：`#` 或 `//` 开头的行。
- 支持的指令（JetBrains 同名指令）：`# @no-redirect`、`# @no-cookie-jar`、`# @timeout <秒>`；另外新增一个 `# @insecure`，作用是忽略 TLS 证书错误。
- 请求体可以用 `< ./path/to/file.json` 从文件读取。

### 3.2 项目结构

任意一个文件夹都可以是项目，不需要专门的项目文件。打开的那个目录就是项目根目录；CLI 从当前目录往上找，找到包含 `http-client.env.json` 或 `.callme/` 的目录就当作根目录，都找不到就用当前目录。

```
my-api/
├── http-client.env.json           # 各环境的变量，提交到 git
├── http-client.private.env.json   # 密钥和 token，加入 .gitignore
├── auth/login.http
├── users/users.http
└── .callme/                       # 本地状态，加入 .gitignore
    ├── state.json                 # 当前环境、窗口布局、打开的标签页
    ├── cookies.txt                # cookie jar（libcurl 的 Netscape 格式）
    └── history/<请求路径>/<时间戳>.json
```

第一次在项目里运行时，callme 检查 `.gitignore`。如果里面没有 `.callme/` 和 `http-client.private.env.json`，就提示用户添加，不会自动修改文件。

### 3.3 环境文件

沿用 JetBrains 的格式：

```json
{
  "$shared": { "apiVersion": "v1" },
  "dev":     { "baseUrl": "https://dev.api.example.com", "user": "ada" },
  "staging": { "baseUrl": "https://staging.api.example.com", "user": "ada" }
}
```

- 每个顶层键是一个环境；`$shared` 里的变量所有环境都能用。
- 私有文件的结构相同，按环境存放密钥。
- **同名变量的优先级：私有 > 环境 > `$shared`**。CLI 的 `--var` 临时覆盖，优先级最高。
- **来自私有文件的值都算密钥**，输出时打码（见 4.3）。

### 3.4 历史记录

每次执行写一个 JSON 文件：解析后的请求、响应（状态码、响应头、响应体）、分阶段耗时、使用的环境、时间。每个请求只保留最近 50 条。响应体超过 10 MB 时，单独存成旁边的文件，不嵌进 JSON 里。

## 4. CLI 与 MCP

### 4.1 命令

| 命令 | 作用 |
|---|---|
| `callme run <文件>[#名字或序号]` | 执行请求；文件里只有一个请求时，可以不写 `#` 后面的部分 |
| `callme run ... --env <环境> --var k=v` | 选择环境、临时覆盖变量；不指定环境时，用 `.callme/state.json` 里上次选的那个 |
| `callme ls` | 列出项目里所有请求：文件、名字、方法、URL |
| `callme env [环境]` | 列出环境和变量，密钥打码 |
| `callme history <文件>#<名字> [-n 5]` | 查看最近几次的响应 |
| `callme import` | 从标准输入读入 curl 或 fetch 代码，转成 `.http` 后输出到标准输出；`-o <文件>` 写入文件 |
| `callme help format` | 输出 `.http` 语法的简要说明，给 AI 看 |
| `callme mcp` | 以 stdio 方式启动 MCP server |

### 4.2 输出

- 在终端里运行时给人看：带颜色，格式化好的响应。
- 输出不是终端时（被程序调用）自动输出 JSON；`--format human|json` 可以强制指定。

```json
{
  "request":  { "file": "auth/login.http", "name": "login", "env": "dev",
                "method": "POST", "url": "https://dev.api.example.com/auth/login", "headers": {} },
  "response": { "status": 200, "headers": {}, "body": {}, "bodyTruncated": false, "size": 1269 },
  "timing":   { "dns": 3, "connect": 9, "tls": 18, "ttfb": 55, "download": 10, "total": 95 },
  "historyFile": ".callme/history/auth/login.http/login/20261104T143208.json"
}
```

- 响应体是 JSON 时，直接嵌成对象；否则是字符串；二进制内容只给出类型和大小。
- 响应体超过 `--max-body`（默认 32 KB）就截断，并且 `bodyTruncated: true`，完整内容在 `historyFile` 里。
- 出错时同样输出 JSON：`{"error": {"kind": "...", "message": "...", "hint": "..."}}`。

### 4.3 退出码与打码

- `0`：拿到了响应，包括 4xx 和 5xx。
- `1`：网络错误、超时、TLS 错误。
- `2`：文件解析失败、变量没定义、找不到请求等使用错误。
- 加 `--fail` 时，4xx 和 5xx 也返回 `1`。
- 来自私有文件的值，在 `request` 回显和 `callme env` 里一律显示成 `***`。响应体里的内容不打码，因为那是服务器返回的数据。

### 4.4 MCP 工具

只提供读取和执行，不提供写入（AI 用自己的工具改文件就行）：

| 工具 | 参数 | 返回 |
|---|---|---|
| `list_requests` | 无 | 和 `callme ls` 相同 |
| `run_request` | `file`、`name?`、`env?`、`vars?` | 和 `callme run` 的 JSON 相同 |
| `get_history` | `file`、`name?`、`limit?` | 和 `callme history` 相同 |
| `list_envs` | 无 | 和 `callme env` 相同，密钥打码 |

## 5. GUI

设计稿：https://claude.ai/artifact/Jnu3TcGtoqcaLqKixQnwsn （四张图：深色主界面、浅色主界面加悬停卡片、⌘K 命令面板、环境变量界面）

### 5.1 布局

- **标题栏**：项目名；中间是 `⌘K` 命令面板入口；右边是 MCP 状态和环境切换。
- **左侧栏**：「环境」区固定放两个环境文件；「请求」区按文件夹显示，`.http` 文件展开后直接列出里面的每个请求，用颜色区分方法；底部是最近发出的请求。
- **中间**：标签页 → 请求栏（方法、名字、解析后的完整 URL、复制为 curl、发送）→ 编辑器。
- **右侧**：响应区，见 5.3。
- **底部状态栏**：当前文件、第几个请求、环境、变量数、校验状态。

### 5.2 请求编辑

编辑器是 nsh 的 `Editor`，编辑的就是 `.http` 文本本身。

- **高亮**：core 里的 Rust 高亮器负责着色，请求体按 JSON 规则着色；`{{var}}` 的底色用编辑器的装饰功能实现。
- **当前请求**：光标所在的那个请求，左边显示一道青色竖线；`⌘Enter` 发送的就是它。
- **补全**：输入 `{{` 时补全变量和请求名；行首补全请求头名，`:` 后面补全常用值；URL 按历史记录补全；也会补全本项目里用过的请求头。支持 Tab 接受的灰色补全提示。
- **悬停**：鼠标停在 `{{var}}` 上，显示它在各个环境下的值、当前用的是哪个、定义在哪里。
- **跳转**：⌘ 点击 `{{var}}` 跳到环境文件；⌘ 点击 `{{login.response…}}` 跳到 login 请求。
- **诊断**：JSON 语法错误、变量没定义、引用的请求不存在、请求名重复，都用波浪线标出。
- **折叠**：每个请求一折。
- **快捷键**：`⌘/` 注释或取消注释（相当于临时禁用某一行）、`⌘⇧F` 格式化 JSON、`⌘Enter` 发送。
- **自动补 Content-Type**：请求体是合法 JSON 但没写 `Content-Type` 时，发送前自动补上 `application/json`。补上的请求头会出现在历史记录里，但不改动文件。

### 5.3 响应区

- **顶部**：状态码、耗时、大小、复制。
- **响应体**：用只读的 `Editor` 显示。
  - 可以按 JSON 结构折叠：折叠范围由 core 计算；提供「全部折叠 / L1 / L2 / 全部展开」，快捷键 `⌘K ⌘0`、`⌘K ⌘1`、`⌘K ⌘2`、`⌘K ⌘J`。
  - JSONPath 过滤框。
  - SSE 响应按事件逐条追加显示。
- **响应头**：两列，响应头名和值。
- **耗时**：DNS、建立连接、TLS 握手、等待首字节、下载五段的分段图，以及 HTTP 版本和 TLS 版本。
- **历史**：这个请求最近的执行记录，点开可以查看当时的响应。

### 5.4 环境变量界面

- **中间**：两个标签页，分别编辑 `http-client.env.json` 和 `http-client.private.env.json`。这里也是写文本，同样有补全和诊断。比如某个环境缺了别的环境都有、请求里也用到的变量，就提示警告。
- **右侧「生效变量」**：只用来查看，不能编辑。可以切换环境，列出每个变量的实际值和来源（私有、环境、`$shared`、上次响应）；私有的值显示为 `••••`。

### 5.5 ⌘K 命令面板

- 搜索请求和命令：输入 `>` 只搜命令，输入 `@` 只搜请求。
- **粘贴 curl 或 fetch 代码**：显示转换后的 `.http` 预览。转换时会**自动换成变量**：
  - 去掉 query 后的 URL，如果以当前环境的某个变量值开头（比如 `baseUrl`），就把这段换成 `{{baseUrl}}`；
  - 请求头和请求体里的值，如果和私有文件里的某个值完全相同，就换成对应的变量。

  转换完可以选「新建文件」「追加到当前文件」或「直接发送，不保存」。

### 5.6 外观

- 使用 Longbridge GPUI 设计系统：主题色是青色 `#00B8B8`，用在选中、当前项、焦点上；主要按钮在浅色模式下是近黑色，在深色模式下是白色；控件圆角 6px；界面扁平，不用阴影，只有弹层除外；用细线分隔区域。
- 跟随系统切换深色和浅色。
- 代码用等宽字体，13px。

### 5.7 文件同步

GUI 监听项目目录。文件被外部修改（比如 AI 改的）时：如果编辑器里没有未保存的改动，就直接重新加载；如果有，就提示冲突，让用户选择保留哪一份。GUI 自己的改动停止输入 500 毫秒后自动保存。

## 6. 错误处理

| 场景 | 处理 |
|---|---|
| `.http` 语法错误 | 编辑器里显示波浪线；`run` 返回退出码 2，并给出行号 |
| 变量没定义 | 发送前拦截，提示变量名和当前环境；CLI 返回退出码 2 |
| 引用的请求还没执行过 | 报错，提示「先运行 login」；GUI 里提供一键先运行依赖的请求 |
| 网络错误、超时、TLS 错误 | 响应区显示错误类型和原因，比如证书错误时提示可以加 `# @insecure`；CLI 返回退出码 1 |
| 响应体太大 | GUI 虚拟滚动显示；CLI 截断，完整内容存进历史记录 |
| 环境文件不是合法 JSON | 环境界面里显示错误；运行时报错，并指出是哪个文件、第几行 |
| 文件被外部修改 | 见 5.7 |

## 7. 测试

- **core**：
  - 解析器用一组 `.http` 文件和期望结果做快照测试，覆盖 JetBrains 和 REST Client 的样例；
  - 环境合并和变量替换做单元测试；
  - curl 转换（包括自动换成变量）用一组 curl 样例做测试；
  - JSONPath 做单元测试；
  - 执行层在本地起一个测试 HTTP 服务（用标准库写的小服务）做集成测试，覆盖重定向、超时、SSE、cookie、响应引用和错误类型。
- **cli**：对 JSON 输出做快照测试；退出码和打码逻辑做测试。
- **mcp**：用 stdio 跑一遍完整的调用流程。
- **app**：用 `nsh test` 无界面运行关键流程：打开项目 → 选中请求 → 发送 → 查看响应 → 折叠；以及粘贴 curl → 新建文件。

## 8. 风险与待验证

1. **nsh 混合模式里，Rust 能不能自由发网络请求、开线程、链接 libcurl**：写 GUI 之前先用最小原型验证。如果不行，就退回到 nsh 的 `fetch`，代价是拿不到分阶段耗时，也不能忽略证书错误。
2. **`Editor` 不支持行内提示（inlay hint）**：已经用悬停卡片和请求栏里的完整 URL 替代。
3. **折叠后的显示**：nsh 的折叠行末尾只显示 ` ⋯ `，没法把结尾的括号合在同一行，也不能显示元素个数；以后可以考虑给 nsh 提 PR。
4. **本机还没装 nsh**：用 `brew install longbridge/tap/nsh` 安装。
5. **`.http` 方言的差异**：JetBrains 和 REST Client 在细节上有出入，比如多行 URL、文件引用。以本文档 3.1 节为准，遇到不兼容的写法给出诊断提示。

## 9. 里程碑

1. **M0 验证**：nsh 混合模式原型，跑通 TSX 调用 Rust、Rust 用 libcurl 发请求、`Editor` 使用自定义高亮器。
2. **M1 core + CLI**：解析、环境、执行、历史、导入；`run`、`ls`、`env`、`history`、`import`。
3. **M2 MCP**：`callme mcp`。
4. **M3 GUI**：三栏布局、请求编辑（高亮、补全、诊断）、响应区、环境界面、命令面板。
