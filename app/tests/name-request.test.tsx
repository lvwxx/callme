// Naming a new request with the keyboard in a real window: scripts/test-app.sh tests/name-request.test.tsx --target macos
import { open, test } from "nsh/test";

test("names a new request with the keyboard", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900 });
  await app.press("label:新建请求");
  await app.wait(500);
  await app.screenshot("screenshots/9-naming.png");
  await app.keyboard.press("mod+a");
  for (const k of ["a", "g", "e", "n", "t", "s"]) await app.keyboard.press(k);
  await app.keyboard.press("enter");
  await app.wait(800);
  await app.screenshot("screenshots/9-named.png");
  await app.waitFor("label:GET agents", { timeout: 3_000 });
});
