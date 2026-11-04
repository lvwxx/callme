// Pictures of the app on a real platform: scripts/test-app.sh tests/screenshot.test.tsx --target macos
// (headless runs draw nothing: the pictures come from a real window).
import { open, test } from "nsh/test";

test("screenshot", async () => {
  const app = await open("../App.tsx", { width: 1280, height: 760 });
  await app.wait(1500);
  await app.screenshot("screenshots/open.png");
  await app.press("text:发送");
  await app.wait(3000);
  await app.screenshot("screenshots/after.png");
});
