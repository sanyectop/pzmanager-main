import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";

const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("Lua mods expose a drag handle that starts a reorder without toggling selection", () => {
  assert.match(appSource, /className=\{`pz-lua-mod-drag-handle/);
  assert.match(
    appSource,
    /onPointerDown=\{\(event\) => beginLuaModReorder\(event, mod\)\}/,
  );
  // 拖动柄自己吞掉 click，否则行点击会把这一行选中。
  assert.match(
    appSource,
    /onClick=\{\(event\) => event\.stopPropagation\(\)\}\s*\n?\s*onPointerDown=\{\(event\) => beginLuaModReorder/,
  );
  // 只有「按加载顺序显示」时才允许拖动；按名称排序时拖动没有意义。
  assert.match(
    appSource,
    /function beginLuaModReorder\([\s\S]*?if \(event\.button !== 0 \|\| luaModSortMode !== "load"\) return;/,
  );
  assert.match(appSource, /data-lua-mod-sortable=\{reorderable \? "true" : undefined\}/);
  assert.match(appSource, /data-lua-mod-id=\{mod\.id\}/);
  assert.match(cssSource, /\.pz-lua-mod-drag-handle\s*\{[\s\S]*?cursor: grab;/);
  assert.match(cssSource, /\.pz-lua-mod-row\.is-reordering\s*\{/);
});

test("a multi-selection is dragged as one block that keeps its inner order", () => {
  // 抓住的行若在多选里，整组选中（且已启用、非加载器）的模组一起搬。
  assert.match(
    appSource,
    /const selectedIds = ids\.filter\(\(id\) => \{[\s\S]*?selectedLuaModKeys\.has\(rowKey\),?[\s\S]*?\}\);/,
  );
  assert.match(
    appSource,
    /const group = selectedLuaModKeys\.has\(luaModKey\(mod\)\) && selectedIds\.length > 1\s*\?\s*selectedIds\s*:\s*\[mod\.id\];/,
  );
  // 落点行不属于这一组才算锚点；整组用 moveModIdBlock 插入，组内相对顺序由当前加载顺序决定。
  assert.match(
    appSource,
    /\.filter\(\(row\) => !groupKeys\.has\(row\.dataset\.luaModKey \?\? ""\)\)/,
  );
  assert.match(
    appSource,
    /moveModIdBlock\(drag\.ids, drag\.group, previous\.id, true\)[\s\S]{0,200}moveModIdBlock\(drag\.ids, drag\.group, next\.id, false\)/,
  );
  // 预览顺序与落盘顺序都基于同一份 ids，拖动中整组连续排列。
  assert.match(appSource, /setLuaReorder\(\{ key: drag\.key, ids, group: drag\.group \}\);/);
  assert.match(appSource, /luaReorder\?\.group\.some\(/);
  assert.match(appSource, /selectedEnabledLuaModCount/);
});

test("the whole selection follows the pointer 1:1", () => {
  // 按下时把整组行都收进 drag.rows（单行时只有这一行），并记下各自相对指针的偏移和高度。
  assert.match(
    appSource,
    /return \{\s*element,\s*grabOffset: event\.clientY - element\.getBoundingClientRect\(\)\.top,\s*height: element\.offsetHeight,\s*\};/,
  );
  // 坐标统一用内容坐标：offsetTop 相对列表，不受 transform / 滚动影响。
  assert.match(
    appSource,
    /const contentOrigin = origin \? origin\.getBoundingClientRect\(\)\.top - origin\.scrollTop : 0;/,
  );
  assert.match(appSource, /const blockTop = drag\.lastClientY - contentOrigin - drag\.blockTopDelta;/);
  assert.match(
    appSource,
    /entry\.element\.style\.transform = `translateY\(\$\{stackedTop - entry\.element\.offsetTop\}px\)`;/,
  );
  assert.match(
    appSource,
    /blockTopDelta: rows\[grabbedIndex\]\.grabOffset \+ aboveHeight,/,
  );
  // 落点判断同样用 offsetTop（布局位置，行正在播放 FLIP 也不受影响）。
  assert.match(
    appSource,
    /id: row\.dataset\.luaModId \?\? "",\s*top: row\.offsetTop,\s*height: row\.offsetHeight,/,
  );
  assert.match(appSource, /const next = rows\.find\(\(row\) => blockTop <= row\.top \+ row\.height \/ 2\);/);
  // 拖动期间整组行不挂 CSS 过渡，位移完全跟手。
  assert.match(appSource, /element\.style\.transition = "none";/);
  // 松手后不立即清位移：交给 layout effect，等最终顺序提交后再滑回槽位。
  assert.match(
    appSource,
    /luaDropRowsRef\.current = drag\.rows;\s*setLuaDropToken\(\(token\) => token \+ 1\);/,
  );
  assert.match(
    appSource,
    /useLayoutEffect\(\(\) => \{\s*const rows = luaDropRowsRef\.current;\s*luaDropRowsRef\.current = null;[\s\S]{0,900}?const delta = from\[index\] - targets\[index\];/,
  );
  assert.match(appSource, /drag\.lastClientY = event\.clientY;/);
  // 按下就先摞好整组，再启动逐帧循环。
  assert.match(
    appSource,
    /setLuaReorder\(\{ key, ids, group \}\);[\s\S]{0,200}?luaDragUpdateRef\.current\(\);[\s\S]{0,80}?luaDragTickRef\.current\(\);/,
  );
});

test("dragging near the list edge auto-scrolls while the block stays under the cursor", () => {
  // 找出真正能滚的祖先当滚动框。
  assert.match(appSource, /function scrollParentOf\(element: HTMLElement\) \{/);
  assert.match(appSource, /scroller,$/m);
  // 靠近上下边缘时按距离加速滚动。
  assert.match(appSource, /const SCROLL_EDGE = 56;/);
  assert.match(appSource, /const SCROLL_MAX_STEP = 18;/);
  assert.match(
    appSource,
    /if \(y < rect\.top \+ SCROLL_EDGE\) \{[\s\S]{0,700}?scroller\.scrollTop \+= delta;/,
  );
  // 拖动期间逐帧跑（指针停住也继续滚），滚动后按最后的指针位置重新对位。
  assert.match(
    appSource,
    /const tick = \(\) => \{[\s\S]*?autoScroll\(drag\);[\s\S]*?if \(scrollTop !== drag\.lastScrollTop\) \{[\s\S]*?update\(drag\);/,
  );
  assert.match(appSource, /luaDragTickRef\.current\(\);/);
  // 停止拖动时把逐帧循环一起停掉。
  assert.match(appSource, /window\.cancelAnimationFrame\(luaDragFrameRef\.current\);/);
  // 任何祖先滚动（滚轮、滚动链、程序滚动）都要重新对位，否则滚轮滚动时会错位。
  assert.match(
    appSource,
    /const handleScroll = \(\) => \{\s*luaDragUpdateRef\.current\(\);\s*\};/,
  );
  assert.match(appSource, /window\.addEventListener\("scroll", handleScroll, true\);/);
  // 内容原点每次都对「列表顶部 − 滚动量」，滚动多少都不会跑偏。
  assert.match(cssSource, /\.pz-lua-mod-list \{[\s\S]{0,300}?position: relative;/);
});

test("rows slide to their new slots instead of teleporting (FLIP)", () => {
  // 渲染期间、DOM 更新前用 offsetTop 抓起点（不受 transform / 滚动影响）。
  assert.match(
    appSource,
    /const luaOrderSignature = \(filteredLuaMods \?\? \[\]\)\.map\(luaModKey\)\.join\("\\u0000"\);/,
  );
  assert.match(
    appSource,
    /if \(luaFlipRef\.current\?\.signature !== luaOrderSignature\) \{[\s\S]{0,400}?tops\.set\(key, row\.offsetTop\);/,
  );
  // 提交后先反向位移、关掉过渡提交起点，同帧再清空 → .pz-mod-row 的 transform 过渡滑回新槽位。
  assert.match(appSource, /const delta = previous - row\.offsetTop;/);
  assert.match(appSource, /row\.style\.transform = `translateY\(\$\{delta\}px\)`;/);
  assert.match(
    appSource,
    /row\.style\.transition = "none";[\s\S]{0,400}?void document\.body\.offsetHeight;[\s\S]{0,400}?row\.style\.transition = "";[\s\S]{0,120}?row\.style\.transform = "";/,
  );
  // 拖动中被抓住的行由指针驱动，FLIP 跳过整组；松手待放开的行也交给收尾 effect。
  assert.match(
    appSource,
    /const draggingKeys = new Set\(\(luaReorder\?\.group \?\? \[\]\)\.map\(\(id\) => id\.toLowerCase\(\)\)\);/,
  );
  assert.match(appSource, /const droppingRows = new Set\(luaDropRowsRef\.current\?\.map\(\(\{ element \}\) => element\) \?\? \[\]\);/);
  assert.match(appSource, /if \(!key \|\| draggingKeys\.has\(key\) \|\| droppingRows\.has\(row\)\) \{\s*return;\s*\}/);
  // 拖动期间不能再把过渡整块关掉（那正是「瞬移」的来源）。
  assert.doesNotMatch(
    cssSource,
    /\.pz-lua-mod-list:has\(\.pz-lua-mod-row\.is-reordering\) \.pz-mod-row \{\s*transition: none;/,
  );
});

test("dragged rows sit above every other row", () => {
  assert.match(
    cssSource,
    /\.pz-lua-mod-row\.is-reordering \{[\s\S]{0,200}?z-index: 40;/,
  );
  assert.match(cssSource, /\.pz-lua-mod-row\.is-reordering-grabbed \{\s*z-index: 41;\s*\}/);
});

test("the list entry animation never replays while rows are moved", () => {
  // React 换位时移动 DOM 节点会让浏览器重播 .pz-lua-mod-list > * 的滑入动画，
  // 所以列表稳定后（以及开始拖动时）要把动画摘掉。
  assert.match(
    appSource,
    /className=\{`pz-lua-mod-list\$\{luaModListSettled \? " is-settled" : ""\}`\}/,
  );
  assert.match(
    appSource,
    /const timer = window\.setTimeout\(\(\) => setLuaModListSettled\(true\), 1000\);/,
  );
  assert.match(
    appSource,
    /suppressRowClickRef\.current = true;\s*\/\/ 保险：即使入场动画还在播，也立刻停掉[\s\S]{0,120}?setLuaModListSettled\(true\);/,
  );
  assert.match(cssSource, /\.pz-lua-mod-list\.is-settled > \* \{\s*animation: none;\s*\}/);
});

test("dropping the dragged row writes the new order once, on pointer up", () => {
  assert.match(
    appSource,
    /const stop = \(commit: boolean\) => \{[\s\S]*?luaReorderCommitRef\.current\(drag\.ids\);/,
  );
  // 撤预览之前先把新顺序写进本地数据：否则列表先回弹到旧顺序，
  // 等后端保存完再滑到新位置，看起来就是「转一圈」。
  assert.match(
    appSource,
    /if \(commit && changed\) \{\s*\/\/ 先把预览顺序写进本地数据[\s\S]{0,200}?applyLuaModOrder\(drag\.ids\);\s*\}\s*setLuaReorder\(null\);/,
  );
  assert.match(
    appSource,
    /function applyLuaModOrder\(ids: string\[\]\) \{\s*setLuaMods\(\(current\) => \{/,
  );
  // 拖动过程中只改预览顺序，不落盘。
  assert.doesNotMatch(
    appSource,
    /const handleMove = \(event: PointerEvent\) => \{[\s\S]{0,2000}save_lua_mod_order/,
  );
  assert.match(
    appSource,
    /async function commitLuaModReorder\(ids: string\[\]\) \{[\s\S]*?invoke<string\[\]>\("save_lua_mod_order", \{\s*modIds: composeLuaModOrder\(ids\),?\s*\}\)/,
  );
  // Escape / pointercancel 放弃这次拖动。
  assert.match(appSource, /if \(event\.key === "Escape"\) stop\(false\);/);
  assert.match(appSource, /window\.addEventListener\("pointercancel", handleCancel\);/);
});

test("manual ordering never counts or moves the loader mods", () => {
  assert.match(
    appSource,
    /function movableLuaModIds\(\) \{[\s\S]*?entry\.enabled && entry\.loadOrder !== null && !entry\.isLoader/,
  );
  assert.match(
    appSource,
    /function composeLuaModOrder\(ids: string\[\]\) \{[\s\S]*?entry\.isLoader \? entry\.id : ids\[cursor\+\+\]/,
  );
  // 置顶/置底的边界也只看可排序模组。
  assert.match(appSource, /const orderIds = movableLuaModIds\(\);/);
  assert.match(
    appSource,
    /disabled=\{busy \|\| orderIndex < 0 \|\| orderIndex >= orderIds\.length - 1\}/,
  );
});

test("row buttons move a mod straight to the top or bottom", () => {
  assert.match(appSource, /async function moveLuaModToEdge\(mod: LuaModEntry, edge: "top" \| "bottom"\)/);
  // 从可排序列表里摘出来插到最前/最后，其余模组相对顺序不变。
  assert.match(
    appSource,
    /const target = edge === "top" \? 0 : ids\.length - 1;[\s\S]{0,200}?const \[moved\] = ids\.splice\(index, 1\);\s*ids\.splice\(target, 0, moved\);/,
  );
  // 本地先排好再落盘，列表不会先停一下再滑。
  assert.match(appSource, /applyLuaModOrder\(ids\);\s*try \{\s*const applied = await invoke<string\[\]>\("save_lua_mod_order"/);
  assert.match(appSource, /aria-label=\{`置顶 \$\{mod\.name\}`\}/);
  assert.match(appSource, /aria-label=\{`置底 \$\{mod\.name\}`\}/);
  assert.match(appSource, /void moveLuaModToEdge\(mod, "top"\);/);
  assert.match(appSource, /void moveLuaModToEdge\(mod, "bottom"\);/);
  // 旧的逐格上移/下移入口已经移除。
  assert.doesNotMatch(appSource, /function moveLuaMod\(/);
  assert.doesNotMatch(appSource, /`[上下]移 \$\{mod\.name\}`/);
});

test("a move blocked by requires/loadBefore is reported instead of silently ignored", () => {
  assert.match(
    appSource,
    /function reportLuaModOrderResult\(requested: string\[\], applied: string\[\]\) \{/,
  );
  assert.match(appSource, /前置模组（requires \/ loadBefore）限定了加载顺序。/);
  // 置顶/置底和拖动排序都要报告。
  assert.match(appSource, /const applied = await invoke<string\[\]>\("save_lua_mod_order"[\s\S]{0,300}reportLuaModOrderResult\(ids, applied\);/);
});

test("the backend keeps loaders in the first priority tier", () => {
  assert.match(
    rustSource,
    /let priority =\s*\|id: &str, entry: Option<&LuaModEntry>\| match id\.to_ascii_lowercase\(\)\.as_str\(\) \{\s*_ if entry\.is_some_and\(\|entry\| entry\.is_loader\) => 0,/,
  );
  assert.match(rustSource, /fn keeps_loaders_ahead_of_a_manually_moved_order/);
});
