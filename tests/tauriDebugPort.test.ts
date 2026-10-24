import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const viteConfig = readFileSync(fileURLToPath(new URL("../vite.config.ts", import.meta.url)), "utf8");
const tauriConfig = JSON.parse(
  readFileSync(fileURLToPath(new URL("../src-tauri/tauri.conf.json", import.meta.url)), "utf8"),
) as { build: { devUrl: string } };
const packageJson = JSON.parse(
  readFileSync(fileURLToPath(new URL("../package.json", import.meta.url)), "utf8"),
) as { scripts: Record<string, string> };
const debugConfig = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("../src-tauri/tauri.dev.conf.json", import.meta.url)),
    "utf8",
  ),
) as {
  app: {
    windows: Array<{
      additionalBrowserArgs: string;
    }>;
  };
};

test("configures the Tauri development WebView remote debugging port as 10001", () => {
  assert.equal(
    packageJson.scripts["tauri:dev"],
    "tauri dev --config src-tauri/tauri.dev.conf.json",
  );
  assert.equal(
    debugConfig.app.windows[0]?.additionalBrowserArgs,
    "--remote-debugging-port=10001",
  );
});

test("runs Vite and points Tauri at port 10002", () => {
  assert.match(viteConfig, /port:\s*10002/);
  assert.equal(tauriConfig.build.devUrl, "http://127.0.0.1:10002");
});
