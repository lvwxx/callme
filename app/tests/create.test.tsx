// New requests and environments in a real window: scripts/test-app.sh tests/create.test.tsx --target macos
import { expect, open, test } from "nsh/test";

test("adds a request and an environment", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900 });
  await app.press("label:新建请求");
  await app.waitFor("label:GET 新请求", { timeout: 5_000 });
  await app.press("label:切换环境，当前 dev");
  await app.chooseMenuItem("新建环境…");
  await app.type("id:new-env-input", "staging");
  // The Return key in a real window: the test's key goes to the view with the keyboard focus (the menu's button).
  await app.press("label:创建");
  await app.waitFor("label:切换环境，当前 staging", { timeout: 5_000 });
  await app.wait(500);
  await app.screenshot("screenshots/8-new-env.png");
  expect(app.texts()).toContain("staging");
});
