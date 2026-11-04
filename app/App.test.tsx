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
  await app.press("label:GET stream");
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
  await app.press("label:GET stream");
  await app.keyboard.press("mod+enter");
  await app.waitFor("text:[DONE]", { timeout: 10_000 });
});

test("switches the environment from the title bar", async () => {
  // The menu is the desktop's (nsh/desktop): run as macOS.
  const app = await open("./App.tsx", { width: 1280, height: 800, platform: "macos" });
  await app.press("label:切换环境，当前 dev");
  expect(app.menu()!.map((m) => m.label)).toEqual(["dev", "prod"]);
  await app.chooseMenuItem("prod");
  await app.waitFor("text:https://api.example.invalid/me", { timeout: 5_000 });
  await app.press("label:切换环境，当前 prod");
  await app.chooseMenuItem("dev");
  await app.waitFor("text:环境 dev · 1 个变量", { timeout: 5_000 });
});

test("reloads a file changed outside", async () => {
  const app = await open("./App.tsx", { width: 1280, height: 800 });
  Callme.write(Callme.root(), "api.http", Callme.read(Callme.root(), "api.http") + "\n### 新增\n# @name added\nGET {{baseUrl}}/added\n");
  await app.waitFor("text:added", { timeout: 5_000 });
  await app.waitFor("text:api.http 已被外部修改，已重新加载", { timeout: 5_000 });
});
