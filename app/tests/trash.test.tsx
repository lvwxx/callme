// Moving a file to the Trash goes through the system: scripts/test-app.sh tests/trash.test.tsx --target macos
import { expect, open, test } from "nsh/test";

test("trashes a request file", async () => {
  const app = await open("../App.tsx", { width: 1280, height: 800 });
  await app.press("label:新建请求文件");
  await app.type("id:name-input", "old");
  await app.keyboard.press("enter");
  await app.waitFor("text:old.http", { timeout: 5_000 });
  await app.rightClick("label:old.http");
  await app.chooseMenuItem("移到废纸篓");
  await app.waitFor("text:已把 old.http 移到废纸篓", { timeout: 5_000 });
  expect(app.texts()).not.toContain("old.http");
});
