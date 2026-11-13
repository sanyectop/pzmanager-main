import assert from "node:assert/strict";
import test from "node:test";

import {
  extractSuspectClass,
  formatGameStopDialogBody,
  GAME_LAUNCH_FAIL_TITLE,
  GAME_STOP_DIALOG_TITLE,
  shouldReportSessionEnd,
  stopReasonLabel,
} from "../src/sessionEnd.ts";
import { appSource, cssSource, rustSource } from "./sourceText.ts";

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
  // 启动器自己点「停止游戏进程」强杀：用户主动结束，不弹崩溃报告（也不做停止分析）。
  assert.equal(shouldReportSessionEnd(sessionEnd("finished", "launcherStop")), false);
  assert.equal(shouldReportSessionEnd(sessionEnd("game-exited", "launcherStop")), false);
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
  assert.equal(stopReasonLabel("launcherStop"), "启动器强制结束");
  assert.equal(stopReasonLabel("unknown"), "原因未知");
  assert.equal(stopReasonLabel(null), "");
});

test("listener routes stop events to the crash report and launch failures to the confirm dialog", () => {
  const listener = appSource.slice(
    appSource.indexOf('listen<OverlaySessionUpdate>("overlay-session"'),
    appSource.indexOf('listen<OverlayZbProgress>("overlay-zb-progress"'),
  );
  // 停止事件走崩溃报告（正文在 openCrashReport 里由 formatGameStopDialogBody 排版）。
  assert.ok(listener.includes("shouldReportSessionEnd(update)"), "监听器应调用 shouldReportSessionEnd");
  assert.ok(listener.includes("openCrashReport(update)"), "停止事件应打开崩溃报告");
  assert.doesNotMatch(
    listener,
    /shouldReportSessionEnd[\s\S]*showConfirm\(/,
    "停止事件不应再走通用确认框",
  );
  // 会话失败（注入失败等）保持确认框。
  assert.ok(listener.includes("title: GAME_LAUNCH_FAIL_TITLE"), "会话失败应有专属模态框标题");
  assert.ok(listener.includes("showConfirm({"), "启动失败应弹确认框");
  assert.ok(!listener.includes("showToast("), "停止/失败报错不应再走 toast");
  // 正文排版与标题常量都在 openCrashReport 里用到。
  const opener = appSource.slice(
    appSource.indexOf("function openCrashReport("),
    appSource.indexOf("async function askConfirm("),
  );
  assert.ok(opener.includes("formatGameStopDialogBody(update)"), "正文应拆分结果与诊断");
  assert.ok(opener.includes("title: GAME_STOP_DIALOG_TITLE"), "崩溃报告应使用停止弹窗标题");
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

test("extracts the suspect class only from missing-class diagnoses", () => {
  // 注入项相关：后端点名缺失的类 → 拿去反查模组。
  assert.equal(
    extractSuspectClass(
      "游戏进程已退出。 诊断：游戏缺少类 zombie.util.list.PZArrayList——几乎总是「游戏版本与模组不匹配」。",
    ),
    "zombie.util.list.PZArrayList",
  );
  // 模组自己的二进制名（含 $ 内部类）也完整取出。
  assert.equal(
    extractSuspectClass("游戏进程已退出。 诊断：游戏缺少类 dgveyes.DGVEReach$Inner 等。"),
    "dgveyes.DGVEReach$Inner",
  );
  // 与注入项无关的诊断：内存不足 / 笼统异常 / 原生崩溃 / 外部结束都不反查。
  assert.equal(
    extractSuspectClass("游戏进程已退出。 诊断：游戏内存不足（OutOfMemoryError）。"),
    null,
  );
  assert.equal(
    extractSuspectClass("游戏进程已退出。 诊断：游戏在加载中抛出未处理异常后退出。"),
    null,
  );
  assert.equal(
    extractSuspectClass("游戏进程已退出。 诊断：游戏原生崩溃（EXCEPTION_ACCESS_VIOLATION）。"),
    null,
  );
  assert.equal(extractSuspectClass("游戏进程已退出。"), null);
});

test("crash report lists suspect jar mods in a scrollable bottom section", () => {
  // 停止事件交给崩溃报告弹窗，而不是通用确认框。
  assert.match(appSource, /function openCrashReport\(/);
  assert.match(appSource, /<CrashReportDialog /);
  const listener = appSource.slice(
    appSource.indexOf('listen<OverlaySessionUpdate>("overlay-session"'),
    appSource.indexOf('listen<OverlayZbProgress>("overlay-zb-progress"'),
  );
  assert.ok(listener.includes("openCrashReport(update)"), "停止事件应打开崩溃报告");
  // 诊断出注入项时才会反查，命令与返回类型都已接线。
  assert.match(appSource, /find_jar_mods_with_injection_item/);
  assert.match(rustSource, /pub async fn find_jar_mods_with_injection_item/);
  // 弹窗底部清单：独立滚动区 + 选中态标签。
  assert.match(cssSource, /\.pz-crash-report-suspect-list \{[^}]*max-height[^}]*overflow-y: auto/s);
  assert.match(appSource, /pz-crash-report-suspect-list/);
  assert.match(appSource, /没有找到引用该注入项的 JAR 模组/);
});

test("crash report dialog plays the shared exit animation on close", () => {
  // 关闭走「先播 is-closing，动画结束再卸载」，与确认框同一套时长。
  assert.match(appSource, /function closeCrashReport\(\)/);
  assert.match(appSource, /setCrashReportClosing\(true\)/);
  assert.match(appSource, /RESOURCE_DIALOG_EXIT_MS\);/);
  assert.match(appSource, /closing=\{crashReportClosing\}/);
  assert.match(appSource, /className=\{`pz-resource-file-overlay\$\{closing \? " is-closing" : ""\}`\}/);
  // 重复关闭（Esc 连按 / 遮罩）在退场中不再重排定时器。
  assert.match(appSource, /if \(!crashReport \|\| crashReportClosing\) \{\s*return;/);
});
