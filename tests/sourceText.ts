import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));

function collect(directory: string, extensions: string[]): string {
  const base = join(root, directory);
  const files: string[] = [];
  const walk = (current: string) => {
    for (const name of readdirSync(current).sort()) {
      const full = join(current, name);
      if (statSync(full).isDirectory()) {
        walk(full);
      } else if (extensions.some((extension) => name.endsWith(extension))) {
        files.push(full);
      }
    }
  };
  walk(base);
  return files.map((file) => readFileSync(file, "utf8")).join("\n");
}

/** 前端源码的拼接文本：App.tsx 拆成多个文件后，测试不再依赖某个具体文件。 */
export const appSource = collect("src", [".ts", ".tsx"]);
/** Rust 后端的拼接文本：lib.rs 拆成多个模块后，测试不再依赖 lib.rs 单文件。 */
export const rustSource = collect("src-tauri/src", [".rs"]);
export const cssSource = readFileSync(join(root, "src/App.css"), "utf8");
export const guideSource = readFileSync(join(root, "docs/ui-style-guide.md"), "utf8");
