// A picture of the filter's placeholder: scripts/test-app.sh tests/placeholder.test.tsx --target macos
import { open, test } from "nsh/test";

test("placeholder", async () => {
  const app = await open("../App.tsx", { width: 1440, height: 900, colorScheme: "light" });
  await app.wait(1000);
  await app.screenshot("screenshots/7-placeholder.png");
});
