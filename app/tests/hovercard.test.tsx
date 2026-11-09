// The hover card stays while the mouse moves onto it: scripts/test-app.sh tests/hovercard.test.tsx --target macos
import { open, test } from "nsh/test";

test("hover card", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900 });
  await app.wait(1000);
  await app.press("label:GET 用户");
  await app.wait(300);
  await app.hover("text:GET {{baseUrl}}/who?id={{token.response.body.$.user.id}}");
  await app.wait(1200);
  await app.hover("text:响应引用");
  await app.wait(800);
  if (!app.shows("text:响应引用")) throw new Error("card closed while the mouse was on it");
  await app.hover("text:发送");
  await app.wait(800);
  if (app.shows("text:响应引用")) throw new Error("card stayed after the mouse left");
});

test("the hover card's footer goes to the definition", async () => {
  // Narrow enough that the middle of `X-Secret: {{secret}}` is on the variable.
  const app = await open("../App.tsx", { width: 1070, height: 800 });
  await app.wait(1000);
  await app.press("label:GET 用户");
  await app.wait(300);
  await app.hover("text:X-Secret: {{secret}}");
  await app.wait(1200);
  await app.press("label:跳转到 http-client.env.json:4");
  await app.waitFor("text:生效变量");
});
