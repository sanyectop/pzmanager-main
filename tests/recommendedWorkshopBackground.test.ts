import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);

test("adds the recommended Project Anime Backgrounds workshop submenu", () => {
  const subscriptionMenuStart = appSource.indexOf("<Text flex=\"1\">创意工坊订阅</Text>");
  const subscriptionMenuEnd = appSource.indexOf("</Menu.Content>", subscriptionMenuStart);
  const subscriptionMenu = appSource.slice(subscriptionMenuStart, subscriptionMenuEnd);
  const scanButtonIndex = subscriptionMenu.indexOf("查找创意工坊背景目录");
  const recommendedItemIndex = subscriptionMenu.indexOf("推荐的创意工坊项目");

  assert.ok(scanButtonIndex >= 0);
  assert.ok(recommendedItemIndex > scanButtonIndex);
  assert.ok(subscriptionMenu.includes("Project Anime Backgrounds"));
  assert.ok(subscriptionMenu.includes("L4D2丧尸围攻 主菜单壁纸"));
  assert.ok(subscriptionMenu.includes("Weeb Background"));
  assert.ok(subscriptionMenu.includes("Blue Archive Visual Innovation"));
  assert.ok(subscriptionMenu.includes("Dying Light Main Menu Background"));
  assert.match(subscriptionMenu, /<Menu\.TriggerItem>[\s\S]*推荐的创意工坊项目/);
  assert.match(appSource, /steamWorkshopUrl\(workshopId\)/);
  assert.match(appSource, /onSelect=\{\(\) => onOpenRecommendedWorkshop\("3388808256"\)\}/);
  assert.match(appSource, /onSelect=\{\(\) => onOpenRecommendedWorkshop\("3791676161"\)\}/);
  assert.match(appSource, /onSelect=\{\(\) => onOpenRecommendedWorkshop\("3410321790"\)\}/);
  assert.match(appSource, /onSelect=\{\(\) => onOpenRecommendedWorkshop\("3141620300"\)\}/);
  assert.match(appSource, /onOpenRecommendedWorkshop=\{openRecommendedWorkshop\}/);
});
