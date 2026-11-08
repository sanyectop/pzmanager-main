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
  // tauri:dev 先重建覆盖层 DLL：workspace 产物只在显式构建时更新，
  // 不挂在 dev 前置的话，注入用的就是旧副本（11-01~11-06 的"改动不生效"即此因）。
  assert.equal(
    packageJson.scripts["tauri:dev"],
    "npm run build:overlay && tauri dev --config src-tauri/tauri.dev.conf.json",
  );
  assert.equal(packageJson.scripts["build:overlay"], "cargo build --release --target x86_64-pc-windows-msvc --manifest-path overlay-dll/Cargo.toml");
  // 调试端口 + 允许无手势自动播放（背景音乐默认播放需要它，WebView2 默认按手势策略拦截）
  const debugArgs = debugConfig.app.windows[0]?.additionalBrowserArgs ?? "";
  assert.match(debugArgs, /--remote-debugging-port=10001/);
  assert.match(debugArgs, /--autoplay-policy=no-user-gesture-required/);
});

test("runs Vite and points Tauri at port 10002", () => {
  assert.match(viteConfig, /port:\s*10002/);
  assert.equal(tauriConfig.build.devUrl, "http://127.0.0.1:10002");
});
