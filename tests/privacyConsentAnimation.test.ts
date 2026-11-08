import assert from "node:assert/strict";
import test from "node:test";

import { appSource, cssSource } from "./sourceText.ts";

test("隐私协议同意后播放退场动画再卸载", () => {
  // 同意 → 先置 closing，170ms 后才隐藏；不能直接 setShowPrivacyConsent(false)。
  const agree = appSource.slice(
    appSource.indexOf("async function agreePrivacyPolicy()"),
    appSource.indexOf("async function disagreePrivacyPolicy()"),
  );
  assert.ok(agree.length > 0, "缺少 agreePrivacyPolicy");
  assert.match(agree, /setPrivacyClosing\(true\)/);
  assert.match(agree, /window\.setTimeout\(\(\) => \{\s*setShowPrivacyConsent\(false\);\s*setPrivacyClosing\(false\);/s);
  assert.match(agree, /RESOURCE_DIALOG_EXIT_MS/);
  assert.doesNotMatch(
    agree,
    /setPrivacyConsentedVersion\(version\);\s*setShowPrivacyConsent\(false\);/,
    "同意后不应瞬间卸载弹窗",
  );

  // 派生状态的 effect 不能在退场期间抢先关掉弹窗。
  assert.match(
    appSource,
    /if \(!privacyClosing\) \{\s*setShowPrivacyConsent\(false\);\s*\}/s,
  );
  assert.match(appSource, /\}, \[privacyConsentedVersion, launcherUpdate, privacyClosing\]\);/);

  // 遮罩挂 is-closing，CSS 里遮罩与面板各自播退场动画。
  assert.match(appSource, /className=\{`pz-privacy-overlay\$\{privacyClosing \? " is-closing" : ""\}`\}/);
  assert.match(cssSource, /\.pz-privacy-overlay\.is-closing \{\s*animation: pz-resource-overlay-out var\(--pz-dur-exit\)/s);
  assert.match(cssSource, /\.pz-privacy-overlay\.is-closing \.pz-privacy-dialog \{\s*animation: pz-resource-dialog-out var\(--pz-dur-exit\)/s);
});

test("撤销同意时清掉残留的退场状态", () => {
  const revoke = appSource.slice(
    appSource.indexOf("async function revokePrivacyConsent()"),
    appSource.indexOf("async function pickPreviewImage"),
  );
  assert.ok(revoke.length > 0, "缺少 revokePrivacyConsent");
  assert.match(revoke, /setPrivacyClosing\(false\);/);
});
