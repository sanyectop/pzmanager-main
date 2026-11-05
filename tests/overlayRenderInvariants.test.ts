import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const overlayLib = readFileSync(
  fileURLToPath(new URL("../overlay-dll/src/lib.rs", import.meta.url)),
  "utf8",
);
const overlayUi = readFileSync(
  fileURLToPath(new URL("../overlay-dll/src/ui.rs", import.meta.url)),
  "utf8",
);
const overlaySession = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/overlay_session.rs", import.meta.url)),
  "utf8",
);
const overlayCore = readFileSync(
  fileURLToPath(new URL("../overlay-core/src/lib.rs", import.meta.url)),
  "utf8",
);

/**
 * 这两条不变量是"红屏无字"事故的直接产物，回归代价很高，因此用测试锁住：
 *
 * 1. 界面图没就绪时一帧都不画（绝不呈现裸背景帧）。
 *    事故链：加载期游戏只换帧 1 次，这次被画成纯红并呈现，随后游戏长时间不再呈现，
 *    纯红就一直挂在屏幕上直到主菜单。只要不画裸帧，这个现象不可能再出现。
 * 2. GDI BitBlt 通道的开关基于"最近是否换帧"的时间窗，而不是"是否换过帧"的布尔值。
 *    布尔门控会让"游戏几乎不换帧"的机器彻底失去唯一能出画的通道。
 */
test("overlay never presents a bare background frame", () => {
  // paint_red_frame 的第一件事就是检查界面图是否可用，不可用直接返回。
  const paintStart = overlayLib.indexOf("unsafe fn paint_red_frame()");
  assert.ok(paintStart > 0, "找不到 paint_red_frame");
  const paintEnd = overlayLib.indexOf("fn draw_ui_texture()", paintStart);
  assert.ok(paintEnd > paintStart, "找不到 paint_red_frame 的结束位置");
  const paintBody = overlayLib.slice(paintStart, paintEnd);
  assert.match(
    paintBody,
    /if !crate::ui::image_available\(\)/,
    "paint_red_frame 必须先检查界面图是否就绪",
  );
  assert.match(
    paintBody,
    /return;/,
    "界面图未就绪时必须提前返回（不放行 glClear）",
  );
  // glClear 必须出现在这个守卫之后。
  assert.ok(
    paintBody.indexOf("glClear(GL_COLOR_BUFFER_BIT)") > paintBody.indexOf("image_available()"),
    "glClear 不能被放到守卫之前",
  );
});

test("bitblt gate uses a recency window, not a one-shot flag", () => {
  // ui 侧提供时间窗查询。
  assert.match(overlayUi, /pub fn swap_recently\(window_ms: u64\) -> bool/);
  assert.match(overlayUi, /static LAST_SWAP_MS: AtomicU64/);
  // note_swap 必须记录时间戳，而不只是累加计数。
  const noteStart = overlayUi.indexOf("pub fn note_swap()");
  assert.ok(noteStart > 0, "找不到 note_swap");
  assert.match(
    overlayUi.slice(noteStart, noteStart + 400),
    /LAST_SWAP_MS\.store/,
    "note_swap 必须更新时间戳",
  );
  // 界面线程按时间窗决定是否 BitBlt；旧的 swap_seen 布尔门控不应再出现。
  assert.match(overlayUi, /if !swap_recently\(500\)/);
  assert.doesNotMatch(overlayUi, /fn swap_seen\(/, "swap_seen 布尔门控应已移除");
});

test("forced-present mechanism stays removed", () => {
  // wglMakeCurrent 强制呈现是"主菜单闪帧"和"加载期纯红"的根源，不应复活。
  assert.doesNotMatch(overlayLib, /fn install_make_current_hook\(/);
  assert.doesNotMatch(overlayLib, /fn force_present_frame\(/);
  assert.doesNotMatch(overlayLib, /wglGetCurrentDC/);
  // gdi32 备用 Hook 保留（部分机器换帧不走 opengl32），且在 swap hook 启用时安装。
  assert.match(overlayLib, /fn install_gdi_swap_hook\(/);
  assert.match(overlayLib, /install_gdi_swap_hook\(\);/);
});

test("every presented frame is painted; only nested swaps are skipped", () => {
  // 旧实现按「同一 HDC+上下文、5ms 内」去重：帧间隔短于 5ms 时（高帧率加载屏）
  // 会把真正要呈现的独立帧当成重复帧跳过，游戏画面/黑块因此漏出 = 用户看到的闪屏。
  // 现在唯一允许跳过的是「外层 swap 调用的原始实现又走了另一条被挂钩的 swap」。
  assert.match(overlayLib, /fn skip_paint_for_nested_swap\(nested: bool\) -> bool/);
  assert.match(overlayLib, /INSIDE_OUTER_SWAP/);
  assert.match(overlayLib, /fn paint_red_while_active\(\)/);
  // 不再有时间窗去重。
  assert.doesNotMatch(overlayLib, /LAST_PAINT_MS/);
  assert.doesNotMatch(overlayLib, /fn should_paint_for\(/);
  // 外层 wglSwapBuffers 调用原始函数期间打标记，结束后恢复。
  assert.match(overlayLib, /flag\.replace\(true\)/);
  assert.match(overlayLib, /flag\.set\(previous\)/);
  // UI 心跳同时报告换帧/绘制，便于判断是否漏掉某条呈现路径。
  assert.match(overlayUi, /绘制=\{\}/);
});

test("render watchdog ends the overlay when the game renders but console.txt stays stale", () => {
  // 有些机器游戏根本不写 console.txt：所有 [ZB] 判定都会失效，必须靠渲染速率收尾。
  assert.match(overlayUi, /pub const RENDER_REPORT_FILE: &str = "pz_overlay_render.txt"/);
  assert.match(overlayUi, /pub fn report_render_activity\(\)/);
  assert.match(overlayUi, /report_render_activity\(\);/);
  assert.match(overlaySession, /const RENDER_READY_RATE: u64 = 40/);
  assert.match(
    overlaySession,
    /const RENDER_READY_HOLD: Duration = Duration::from_secs\(10\)/,
  );
  // console.txt 迟迟不更新（等待阶段）和「有更新但没有 ZB 行」（主循环）都要能收尾。
  assert.match(overlaySession, /但 console\.txt 没有更新（读不到 \[ZB\] 进度）/);
  assert.match(overlaySession, /游戏已持续渲染 \{\} 秒（本次没有 \[ZB\] 输出）/);
  // 会话结束打一行诊断，反馈里能直接看出断点。
  assert.match(overlaySession, /覆盖层会话详情：ZB 行数=\{\}，阶段=\{\}，advice 分母=\{\}，渲染≈\{\}/);
});

test("any-phase silence fallback is 60 seconds", () => {
  // 联网静默实测最长 41 秒；旧的 120 秒会让加载界面多压 1 分钟。
  assert.match(
    overlayCore,
    /ZB_IDLE_TIMEOUT_ANY_PHASE: Duration = Duration::from_secs\(60\)/,
  );
});
