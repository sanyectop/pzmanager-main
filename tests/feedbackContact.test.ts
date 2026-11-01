import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";

const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("反馈的联系方式是选填项，上传时拼到描述末尾", () => {
  // 后端：命令收 Option<String>，非空时拼成「\n\n联系方式：…」；中转站协议不用改。
  assert.match(
    rustSource,
    /async fn submit_feedback\(\s*description: String,\s*contact: Option<String>,?\s*\)/,
  );
  assert.match(rustSource, /fn compose_report\(description: &str, contact: &str\) -> String \{/);
  assert.match(rustSource, /format!\("\{description\}\\n\\n联系方式：\{contact\}"\)/);
  assert.match(rustSource, /const MAX_CONTACT_CHARS: usize = 200;/);
  assert.match(rustSource, /"description": report,/);
  // 前端：空串按「没填」传 null，不拼进描述。
  assert.match(appSource, /contact: feedbackContact\.trim\(\) \|\| null,/);
  assert.match(appSource, /aria-label="联系方式"/);
  assert.match(appSource, /联系方式（选填，会附在描述末尾，方便回访）/);
  // 提交成功后清空联系方式，避免下次误带。
  assert.match(appSource, /setFeedbackDescription\(""\);\s*setFeedbackContact\(""\);/);
});

test("关于页反馈下方有可展开的已提交反馈编号列表", () => {
  // 后端：提交成功后记一条（id + 时间 + 摘要），最多 50 条，新的在前。
  assert.match(rustSource, /pub\(crate\) struct FeedbackHistoryEntry \{/);
  assert.match(rustSource, /const MAX_HISTORY_ENTRIES: usize = 50;/);
  assert.match(rustSource, /remember_submitted\(&result\.feedback_id, &description, sent_at\);/);
  assert.match(rustSource, /feedback-history\.json/);
  assert.match(rustSource, /fn list_submitted_feedback\(\) -> Result<Vec<feedback::FeedbackHistoryEntry>, String>/);
  assert.match(rustSource, /^\s+list_submitted_feedback,$/m);
  // 前端：可展开按钮 + 行（编号/时间/摘要/复制），进关于页时读一次。
  assert.match(appSource, /invoke<FeedbackHistoryEntry\[\]>\("list_submitted_feedback"\)/);
  assert.match(appSource, /aria-expanded=\{feedbackHistoryOpen\}/);
  assert.match(appSource, /已提交的反馈/);
  assert.match(appSource, /copyFeedbackId\(entry\.id\)/);
  assert.match(
    appSource,
    /if \(activePage !== "settings" \|\| settingsPage !== "about"\) return;\s*void loadFeedbackHistory\(\);/,
  );
  assert.match(cssSource, /\.pz-feedback-history-row \{[\s\S]{0,200}?border: 1px solid var\(--pz-line\);/);
});

test("文字型小按钮两侧留出内边距（.pz-text-button）", () => {
  // 「已提交的反馈」不再写 padding-inline: 0，改为统一的文字按钮样式。
  assert.doesNotMatch(appSource, /\.pz-feedback-history-toggle \{[\s\S]{0,200}?padding-inline: 0/);
  assert.match(
    cssSource,
    /\.pz-app-shell \.chakra-button\.pz-text-button \{\s*padding-inline: 0\.62rem !important;\s*\}/,
  );
  // 同类文字按钮都挂上这个类。
  for (const label of ["已提交的反馈", "复制反馈编号", "重新开启换盘提示", "版本记录"]) {
    assert.ok(appSource.includes(label), `缺少按钮：${label}`);
  }
  assert.ok(
    (appSource.match(/className="pz-text-button"/g) ?? []).length >= 4,
    "同类文字按钮应统一挂 .pz-text-button",
  );
  assert.match(appSource, /className="pz-feedback-history-toggle pz-text-button"/);
});

test("反馈会带上启动器日志（整份日志尾部）", () => {
  // 后端：launcher.log 取整份日志尾部，而不是只切最近一次会话。
  assert.match(rustSource, /crate::launcher_log::full_log_tail\(MAX_LAUNCHER_LOG_BYTES\)/);
  assert.match(rustSource, /pub\(crate\) fn full_log_tail\(max_bytes: usize\) -> Option<\(String, bool\)>/);
  assert.doesNotMatch(rustSource, /recent_session_log/);
  // 弹窗文案要写明会带启动器日志，别让人觉得只传游戏日志。
  assert.match(appSource, /launcher\.log，最近 1 MB/);
});
