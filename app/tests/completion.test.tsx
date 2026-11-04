// A picture of the completion list: scripts/test-app.sh tests/completion.test.tsx --target macos
import { open, test } from "nsh/test";

test("completion", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900 });
  await app.wait(1000);
  await app.type("id:request-editor", "");
  await app.keyboard.press("mod+right");
  await app.keyboard.press("enter");
  await app.type("id:request-editor", "Acc");
  await app.wait(1000);
  await app.screenshot("screenshots/6-completion.png");
});
