// Copy as curl in a real window: scripts/test-app.sh tests/curl.test.tsx --target macos
import { expect, open, test } from "nsh/test";

test("copies the current request as curl", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900 });
  await app.press("label:curl 命令");
  await app.waitFor("label:复制 curl 命令", { timeout: 3_000 });
  await app.wait(300);
  await app.screenshot("screenshots/10-curl.png");
  await app.press("label:复制 curl 命令");
  await app.wait(500);
  expect(app.clipboard() ?? "").toContain("curl 'http://127.0.0.1:");
  // mod+shift+c copies at once.
  await app.keyboard.press("mod+shift+c");
  await app.waitFor("text:已复制为 curl", { timeout: 3_000 });
});
