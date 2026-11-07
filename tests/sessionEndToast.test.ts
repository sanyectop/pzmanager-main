import assert from "node:assert/strict";
import test from "node:test";

import {
  formatGameStopDialogBody,
  GAME_LAUNCH_FAIL_TITLE,
  GAME_STOP_DIALOG_TITLE,
  shouldReportSessionEnd,
  stopReasonLabel,
} from "../src/sessionEnd.ts";
import { appSource, rustSource } from "./sourceText.ts";

function sessionEnd(
  phase: string,
  stopReason?: string | null,
): { phase: string; stopReason?: string | null } {
  return { phase, stopReason };
}

test("in-session and post-session stops with an abnormal reason open the dialog", () => {
  for (const phase of ["finished", "game-exited"]) {
    for (const stopReason of ["javaCrash", "nativeCrash", "externalTerminate", "unknown"]) {
      assert.equal(
        shouldReportSessionEnd(sessionEnd(phase, stopReason)),
        true,
        `${phase}/${stopReason}`,
      );
    }
  }
});

test("normal exit and non-stop endings stay silent", () => {
  // 用户主动正常退出（会话内或会话后）：不弹。
  assert.equal(shouldReportSessionEnd(sessionEnd("finished", "normalExit")), false);
  assert.equal(shouldReportSessionEnd(sessionEnd("game-exited", "normalExit")), false);
  // 覆盖层看门狗/超时收尾：游戏还在跑，后端不给 stopReason，不弹。
  assert.equal(shouldReportSessionEnd(sessionEnd("finished")), false);
  assert.equal(shouldReportSessionEnd(sessionEnd("finished", null)), false);
  // error 阶段由监听器单独弹「游戏未能正常启动」，这里不重复。
  assert.equal(shouldReportSessionEnd(sessionEnd("error", "javaCrash")), false);
  // 进行中的阶段不弹。
  assert.equal(shouldReportSessionEnd(sessionEnd("loading", "javaCrash")), false);
});

test("dialog body separates result, meta and diagnosis", () => {
  const body = formatGameStopDialogBody({
    message:
      "游戏进程已退出。 诊断：游戏缺少类 zombie.util.list.PZArrayList——几乎总是「游戏版本与模组不匹配」。",
    stopReason: "javaCrash",
    exitCode: 1,
  });
  const sections = body.split("\n\n");
  assert.equal(sections.length, 3, body);
  assert.equal(sections[0], "游戏进程已退出。");
  assert.ok(sections[1].includes("停止原因：游戏崩溃（Java 异常）"), sections[1]);
  assert.ok(sections[1].includes("退出码：1（0x00000001）"), sections[1]);
  assert.ok(sections[2].startsWith("诊断："), sections[2]);
  assert.ok(sections[2].includes("zombie.util.list.PZArrayList"), sections[2]);

  // 没有诊断、没有退出码：只剩结果与停止原因，不留空段。
  const minimal = formatGameStopDialogBody({
    message: "游戏进程已退出。",
    stopReason: "externalTerminate",
  });
  assert.equal(minimal.split("\n\n").length, 2, minimal);
  assert.ok(minimal.includes("停止原因：被外部结束"), minimal);
  // 未知停止原因（未来后端新增值）退化为「原因未知」。
  assert.ok(
    formatGameStopDialogBody({ message: "x", stopReason: "futureReason" }).includes("原因未知"),
  );
});

test("stop reason labels cover the backend enum", () => {
  assert.equal(stopReasonLabel("normalExit"), "正常退出");
  assert.equal(stopReasonLabel("javaCrash"), "游戏崩溃（Java 异常）");
  assert.equal(stopReasonLabel("nativeCrash"), "游戏崩溃（原生异常）");
  assert.equal(stopReasonLabel("externalTerminate"), "被外部结束");
  assert.equal(stopReasonLabel("unknown"), "原因未知");
  assert.equal(stopReasonLabel(null), "");
});

test("listener opens confirm dialogs for both stop phases instead of toasting", () => {
  const listener = appSource.slice(
    appSource.indexOf('listen<OverlaySessionUpdate>("overlay-session"'),
    appSource.indexOf('listen<OverlayZbProgress>("overlay-zb-progress"'),
  );
  // 两种停止事件都走模态框（showConfirm），不再 showToast。
  assert.ok(listener.includes("shouldReportSessionEnd(update)"), "监听器应调用 shouldReportSessionEnd");
  assert.ok(listener.includes("title: GAME_STOP_DIALOG_TITLE"), "停止事件应有专属模态框标题");
  assert.ok(listener.includes("title: GAME_LAUNCH_FAIL_TITLE"), "会话失败应有专属模态框标题");
  assert.ok(listener.includes("formatGameStopDialogBody(update)"), "正文应拆分结果与诊断");
  assert.ok(listener.includes("showConfirm({"), "报错应弹模态框");
  assert.ok(!listener.includes("showToast("), "停止/失败报错不应再走 toast");
});

test("console panel knows the game-exited phase", () => {
  // 徽标配色与文案都认识 game-exited（会话后退出）。
  assert.match(appSource, /overlaySession\.phase === "game-exited"/);
  assert.match(appSource, /case "game-exited":/);
  assert.match(appSource, /游戏已退出/);
});

test("backend watches the game after the overlay session ends", () => {
  // 会话收尾后继续监视（stop_reason 为空 = 游戏还在跑）。
  assert.match(rustSource, /fn watch_game_after_overlay/);
  assert.match(rustSource, /outcome\.stop_reason\.is_none\(\)/);
  // 退出后以 game-exited 阶段上报，携带停止原因与退出码。
  assert.match(rustSource, /"game-exited"/);
  // 静默兜底有 console 停更守卫，不再把加载中的空档误判成加载完成。
  assert.match(rustSource, /fn idle_completion_with_console/);
  assert.match(rustSource, /不判定加载完成/);
});
