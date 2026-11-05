# .http 文件语法（callme）

一个 `.http` 文件可以放多个请求，用 `###` 分隔；`###` 后面的文字是标题。

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

## 结构

1. 注释和指令（`#` 或 `//` 开头），可选
2. 请求行：`METHOD URL [HTTP/1.1|HTTP/2]`；只写 URL 时默认 GET
3. query 可以写在缩进的续行里：`    ?page=1`、`    &size=20`
4. 请求头：`Name: value`，每行一个；行首加 `#` 就是禁用这一行
5. 空一行，然后是请求体；`< ./body.json` 从文件读取（相对于这个 .http 文件，原样发送）

## 指令

| 指令 | 作用 |
|---|---|
| `# @name login` | 给请求命名（文件内唯一，不能有空格、`.`、`#`） |
| `# @no-redirect` | 不跟随重定向 |
| `# @no-cookie-jar` | 不读写 cookie（默认会保存在 .callme/cookies.txt） |
| `# @timeout 5` | 超时秒数，默认 30 |
| `# @insecure` | 忽略 TLS 证书错误 |

## 变量

- `{{name}}`：环境变量，来自项目根目录的 `http-client.env.json`（提交）和 `http-client.private.env.json`（密钥，不提交）。
  每个顶层键是一个环境，`$shared` 对所有环境生效。优先级：私有 > 环境 > `$shared`。
- `{{login.response.body.$.token}}`：请求 `login` 最近一次响应里的值（JSONPath）；`{{login.response.headers.Location}}` 取响应头。
  被引用的请求要先运行过。
- `{{$uuid}}`、`{{$timestamp}}`、`{{$isoTimestamp}}`、`{{$randomInt}}`：动态值。

请求体是 JSON 且没写 `Content-Type` 时，会自动加上 `application/json`。
没写 `User-Agent` 时发 `curl/<libcurl 版本> callme/<版本>`（有些网关只放行认识的 User-Agent）；要换就写一行 `User-Agent: …`。

## 不支持

JS 脚本（`> {% ... %}`）、文件变量（`@host = ...`）、multipart 表单。

## 常用命令

```sh
callme ls                                # 列出所有请求
callme run auth/login.http#login -e dev  # 执行；非终端输出 JSON
callme history auth/login.http#login     # 最近的响应
callme env                               # 环境和变量（密钥打码）
pbpaste | callme import -o api.http      # curl / fetch 转成 .http
```
