// Runs against the project in $CALLME_PROJECT (scripts/test-app.sh starts a local server and writes one).
import { expect, open, test } from "nsh/test";

test("runs the selected request", async () => {
  const app = await open("./App.tsx");
  expect(app.texts()).toContain("me");
  await app.press("text:发送");
  await app.waitFor("text:200 OK", { timeout: 10_000 });
});

test("streams server-sent events as they arrive", async () => {
  const app = await open("./App.tsx");
  await app.press("text:stream");
  await app.press("text:发送");
  await app.waitFor('text:{"event":"delta","data":{"i":0,"t":"你"}}', { timeout: 10_000 });
  // The first event shows while the stream still runs.
  expect(app.texts()).not.toContain("200 OK");
  await app.waitFor("text:200 OK", { timeout: 10_000 });
  expect(app.texts()).toContain('{"data":"[DONE]"}');
});
