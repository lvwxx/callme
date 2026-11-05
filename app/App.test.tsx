// Runs against the project in $CALLME_PROJECT: scripts/test-app.sh starts a local server and writes one.
// The completion list and hover cards are overlays headless runs do not list: tests/completion.test.tsx pictures them.
import { expect, open, test } from "nsh/test";
import { Callme } from "callme/native";

test("runs the current request", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  expect(app.texts()).toContain("me");
  await app.press("text:发送");
  await app.waitFor("text:OK", { timeout: 10_000 });
  expect(app.texts()).toContain("200");
});

test("streams server-sent events as they arrive", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:GET 流式");
  await app.press("text:发送");
  await app.waitFor('text:{"i":0,"t":"你"}', { timeout: 10_000 });
  // The first event shows while the stream still runs.
  expect(app.texts()).toContain("发送中");
  await app.waitFor("text:[DONE]", { timeout: 10_000 });
  await app.waitFor("text:OK", { timeout: 10_000 });
});

test("turns a pasted curl command into a request", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:命令面板");
  await app.type("id:palette-input", "curl -H 'Accept: text/plain' https://example.com/v1/orders?a=1");
  await app.waitFor("text:新建 orders.http", { timeout: 5_000 });
});


test("sends with mod+enter", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:GET 流式");
  await app.keyboard.press("mod+enter");
  await app.waitFor("text:[DONE]", { timeout: 10_000 });
});

test("switches the environment from the title bar", async () => {
  // The menu is the desktop's (nsh/desktop): run as macOS.
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.press("label:切换环境，当前 dev");
  expect(app.menu()!.map((m) => m.label)).toEqual(["dev", "prod", "新建环境…", "编辑环境变量"]);
  await app.chooseMenuItem("prod");
  await app.waitFor("text:https://api.example.invalid/me", { timeout: 5_000 });
  await app.press("label:切换环境，当前 prod");
  await app.chooseMenuItem("dev");
  await app.waitFor("text:环境 dev · 2 个变量", { timeout: 5_000 });
});

test("reloads a file changed outside", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  Callme.write(Callme.root(), "api.http", Callme.read(Callme.root(), "api.http") + "\n### 新增\n# @name added\nGET {{baseUrl}}/added\n");
  await app.waitFor("text:added", { timeout: 5_000 });
  await app.waitFor("text:api.http 已被外部修改，已重新加载", { timeout: 5_000 });
});

test("adds a request to the open file", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:新建请求");
  await app.waitFor("text:# @name request", { timeout: 5_000 });
  // Return keeps the name it was given (headless, the field takes the keys once typed into).
  await app.type("id:name-input", "");
  await app.keyboard.press("enter");
  await app.waitFor("label:GET 新请求", { timeout: 5_000 });
  expect(Callme.read(Callme.root(), "api.http")).toContain("# @name request");
});

test("creates and renames request files", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.press("label:新建请求文件");
  await app.type("id:name-input", "users/list");
  await app.keyboard.press("enter");
  await app.waitFor("text:list.http", { timeout: 5_000 });
  expect(app.texts()).toContain("users");
  await app.rightClick("label:list.http");
  await app.chooseMenuItem("重命名…");
  await app.clear("id:name-input");
  await app.type("id:name-input", "users/all");
  await app.keyboard.press("enter");
  await app.waitFor("text:all.http", { timeout: 5_000 });
  expect(Callme.read(Callme.root(), "users/all.http")).toContain("GET {{baseUrl}}/");
});

test("creates an environment", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.press("label:切换环境，当前 dev");
  await app.chooseMenuItem("新建环境…");
  await app.type("id:new-env-input", "staging");
  await app.keyboard.press("enter");
  await app.waitFor("text:staging", { timeout: 5_000 });
  const env = JSON.parse(Callme.read(Callme.root(), "http-client.env.json"));
  expect(env.staging).toEqual({ baseUrl: "", secret: "" });
  await app.waitFor("label:切换环境，当前 staging", { timeout: 5_000 });
  // Back to dev for the tests after this one.
  await app.press("label:切换环境，当前 staging");
  await app.chooseMenuItem("dev");
});

test("goes to a variable's definition", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  // The caret is at the start of `GET {{baseUrl}}/me`: into `baseUrl`, then F12.
  await app.type("id:request-editor", "");
  for (let i = 0; i < 7; i++) await app.keyboard.press("right");
  await app.keyboard.press("f12");
  await app.waitFor("text:生效变量", { timeout: 5_000 });
});

test("runs a request's dependency first", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:GET 依赖：用到 token 的响应");
  await app.press("text:发送");
  await app.waitFor("text:先运行 token，再运行这个请求", { timeout: 5_000 });
  await app.press("text:先运行 token，再运行这个请求");
  await app.waitFor("text:OK", { timeout: 10_000 });
});

test("comments out a header and formats a JSON body", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  // On `me`'s request line: down to `Accept: …`, comment it out.
  await app.type("id:request-editor", "");
  await app.keyboard.press("down");
  await app.keyboard.press("mod+/");
  await app.waitFor("text:# Accept: application/json", { timeout: 5_000 });
  await app.keyboard.press("mod+/");
  // A body on one line, formatted.
  await app.press("label:POST 创建：请求体写在一行里");
  await app.keyboard.press("mod+shift+f");
  await app.waitFor('text:  "a": 1,', { timeout: 5_000 });
  await app.waitFor("text:    {{$uuid}}", { timeout: 5_000 });
});

test("offers to add secrets to .gitignore", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.waitFor("text:.gitignore 里没有 .callme/、http-client.private.env.json：密钥和本地记录可能会被提交到 git", { timeout: 5_000 });
  await app.press("label:加到 .gitignore");
  expect(Callme.read(Callme.root(), ".gitignore")).toContain("http-client.private.env.json");
});

test("warns about an environment missing a variable", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:http-client.env.json");
  // The line depends on the tests before (a new environment rewrites the file).
  await app.waitUntil(() => app.texts().some((t) => /^第 \d+ 行：prod 没有 secret：其他环境有，请求里也用到了$/.test(t)), { timeout: 5_000 });
});

test("folds and unfolds a file's requests", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  expect(app.texts()).toContain("stream");
  // The open file, pressed again: its requests fold away; the arrow brings them back.
  await app.press("label:api.http");
  await app.waitUntil(() => !app.texts().includes("stream"), { timeout: 3_000 });
  await app.press("label:展开 api.http");
  await app.waitFor("text:stream", { timeout: 3_000 });
  await app.press("label:收起 api.http");
  await app.waitUntil(() => !app.texts().includes("stream"), { timeout: 3_000 });
  await app.press("label:展开 api.http");
});

test("renames a request and the references to it", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.rightClick("label:GET 令牌");
  await app.chooseMenuItem("修改请求名（@name）…");
  await app.clear("id:name-input");
  await app.type("id:name-input", "auth");
  await app.keyboard.press("enter");
  // The title stays; the name shows beside it.
  await app.waitFor("text:auth", { timeout: 5_000 });
  const text = Callme.read(Callme.root(), "api.http");
  expect(text).toContain("# @name auth");
  expect(text).toContain("{{auth.response.body.$.user.id}}");
  expect(text).not.toContain("token.response");
});

test("names a new request as it is added", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  await app.press("label:新建请求");
  await app.waitFor("id:name-input", { timeout: 5_000 });
  await app.clear("id:name-input");
  await app.type("id:name-input", "agents");
  await app.keyboard.press("enter");
  await app.waitFor("label:GET agents", { timeout: 5_000 });
});

test("titles a request with any text", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.rightClick("label:GET 用户");
  await app.chooseMenuItem("重命名…");
  await app.clear("id:name-input");
  await app.type("id:name-input", "获取 当前用户（带 token）");
  await app.keyboard.press("enter");
  await app.waitFor("label:GET 获取 当前用户（带 token）", { timeout: 5_000 });
  const text = Callme.read(Callme.root(), "api.http");
  expect(text).toContain("### 获取 当前用户（带 token）\n# @name me");
});

test("the project menu lists recent projects and opens folders", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.press("label:项目");
  const labels = app.menu()!.map((m) => m.label);
  expect(labels).toContain("打开文件夹…");
  expect(labels).toContain("在 Finder 中显示");
  expect(app.menu()!.find((m) => m.checked)?.label.endsWith("/project")).toBe(true);
  await app.closeMenu();
});
