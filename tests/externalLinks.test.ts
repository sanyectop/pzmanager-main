import assert from "node:assert/strict";
import test from "node:test";
import { steamWorkshopFallbackUrl, steamWorkshopUrl } from "../src/externalLinks.ts";

test("builds the Steam Workshop protocol URL", () => {
  assert.equal(
    steamWorkshopUrl("3498201101"),
    "steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=3498201101",
  );
});

test("builds an HTTPS fallback for a Steam Workshop item", () => {
  assert.equal(
    steamWorkshopFallbackUrl("3498201101"),
    "https://steamcommunity.com/sharedfiles/filedetails/?id=3498201101",
  );
});
