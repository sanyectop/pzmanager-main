import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const capabilitySource = readFileSync(
  fileURLToPath(new URL("../src-tauri/capabilities/default.json", import.meta.url)),
  "utf8",
);
const capability = JSON.parse(capabilitySource) as {
  permissions: Array<
    | string
    | {
        identifier?: string;
        allow?: Array<{ url?: string }>;
      }
  >;
};

test("allows Steam openurl workshop links in the opener capability", () => {
  const permission = capability.permissions.find(
    (entry) =>
      typeof entry !== "string" &&
      entry.identifier === "opener:allow-open-url",
  );

  assert.ok(permission && typeof permission !== "string");
  assert.ok(
    permission.allow?.some(
      (entry) =>
        entry.url ===
        "steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=*",
    ),
  );
});
