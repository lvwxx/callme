// Pictures of the app on a real platform: scripts/test-app.sh tests/screenshot.test.tsx --target macos
// (headless runs draw nothing: the pictures come from a real window).
import { open, test } from "nsh/test";

test("screenshots", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900 });
  await app.wait(1500);
  await app.screenshot("screenshots/1-open.png");
  await app.hover("label:项目");
  await app.wait(900);
  await app.screenshot("screenshots/1b-title.png");
  await app.press("text:发送");
  await app.waitFor("text:OK", { timeout: 10_000 });
  await app.wait(500);
  await app.screenshot("screenshots/2-sent.png");
  await app.hover("label:curl 命令");
  await app.wait(900);
  await app.screenshot("screenshots/2b-tooltip.png");
  await app.press("label:GET 流式");
  await app.press("text:发送");
  await app.waitFor("text:[DONE]", { timeout: 10_000 });
  await app.wait(500);
  await app.screenshot("screenshots/3-sse.png");
  await app.press("text:耗时");
  await app.wait(300);
  await app.screenshot("screenshots/3b-timing.png");
  await app.press("text:历史");
  await app.wait(300);
  await app.screenshot("screenshots/3c-history.png");
  await app.press("text:响应体");
  await app.press("label:GET 用户");
  await app.wait(300);
  await app.hover("text:GET {{baseUrl}}/me");
  await app.wait(1200);
  await app.screenshot("screenshots/3d-hover.png");
  await app.setColorScheme("dark");
  await app.wait(800);
  await app.screenshot("screenshots/3e-dark.png");
  await app.setColorScheme("light");
  await app.press("label:http-client.env.json");
  await app.wait(800);
  await app.screenshot("screenshots/4-env.png");
  await app.press("label:命令面板");
  await app.type("id:palette-input", "curl -H 'Accept: text/plain' https://example.com/v1/orders?a=1&b=2");
  await app.wait(800);
  await app.screenshot("screenshots/5-palette.png");
});
