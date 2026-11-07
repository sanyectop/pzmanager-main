/** 光影包配置弹窗的纯逻辑：控件类型推断、初始值、预设预填、覆写 diff。
 *  无副作用，供 tests/shaderpackConfigUi.test.ts 直接覆盖。 */
import type {
  ShaderpackOptionInfo,
  ShaderProfileMeta,
  ShaderProfileToken,
} from "./app/types";

/** 弹窗里单个参数用哪种控件。 */
export type ShaderpackControlKind = "select" | "slider" | "toggle" | "text";

const BOOLEAN_VALUES = new Set(["true", "false"]);

/** 预设下拉里的特殊项：不覆写预设（live 文件里保持原样）。 */
export const NO_PROFILE = "";

/** 控件类型：
 * - 滑块且观测到 ≥2 个取值 → 当枚举下拉处理（OptiFine 滑块常配离散档位）；
 * - 滑块其余情况 → 连续滑条；
 * - 取值都是 true/false → 开关；
 * - 有 ≥2 个观测值 → 枚举；
 * - 其余 → 文本输入。 */
export function controlKindForOption(option: ShaderpackOptionInfo): ShaderpackControlKind {
  if (option.slider && option.values.length >= 2) return "select";
  if (option.slider) return "slider";
  if (
    option.values.length > 0 &&
    option.values.every((value) => BOOLEAN_VALUES.has(value.toLowerCase()))
  ) {
    return "toggle";
  }
  if (option.values.length > 1) return "select";
  return "text";
}

/** 弹窗里每个参数的初始值：live 当前值优先，其次包默认值，最后空串。 */
export function initialOptionValue(option: ShaderpackOptionInfo): string {
  return option.liveValue ?? option.defaultValue ?? "";
}

/** 弹窗打开时的整份初始值表（保存时按它算 diff）。 */
export function initialOverrideValues(options: ShaderpackOptionInfo[]): Record<string, string> {
  return Object.fromEntries(options.map((option) => [option.name, initialOptionValue(option)]));
}

/** 选预设后预填参数值：预设里的 token 覆盖当前值，包里没有的参数忽略。 */
export function applyProfileToValues(
  options: ShaderpackOptionInfo[],
  profile: ShaderProfileMeta | null,
  current: Record<string, string>,
): Record<string, string> {
  const next = { ...current };
  if (!profile) return next;
  const tokenByOption = new Map<string, ShaderProfileToken>(
    profile.tokens.map((token) => [token.option, token]),
  );
  for (const option of options) {
    const token = tokenByOption.get(option.name);
    if (token) next[option.name] = token.value;
  }
  return next;
}

/** 保存时只保留相对初始值真正改过的参数，少写键 = 少动 Viewpoint 的文件。 */
export function collectOverrideValues(
  options: ShaderpackOptionInfo[],
  initial: Record<string, string>,
  current: Record<string, string>,
): Record<string, string> {
  const changed: Record<string, string> = {};
  for (const option of options) {
    const value = current[option.name] ?? "";
    if (value !== (initial[option.name] ?? "")) {
      changed[option.name] = value;
    }
  }
  return changed;
}

/** 滑条值是否可以当数字处理（不能就退回文本输入）。 */
export function isNumericValue(value: string): boolean {
  return value.trim() !== "" && Number.isFinite(Number(value));
}

// ---------------------------------------------------------------------------
// MC § 彩字 / 格式码解析（光影包 lang 标签里可能出现）
// ---------------------------------------------------------------------------

/** MC 颜色码 → 显示色（Java 版标准 16 色）。 */
const MC_COLOR_BY_CODE: Record<string, string> = {
  "0": "#000000",
  "1": "#0000AA",
  "2": "#00AA00",
  "3": "#00AAAA",
  "4": "#AA0000",
  "5": "#AA00AA",
  "6": "#FFAA00",
  "7": "#AAAAAA",
  "8": "#555555",
  "9": "#5555FF",
  a: "#55FF55",
  b: "#55FFFF",
  c: "#FF5555",
  d: "#FF55FF",
  e: "#FFFF55",
  f: "#FFFFFF",
};

/** MC 格式码 → 样式标记。 */
const MC_FORMAT_BY_CODE: Record<string, "bold" | "italic" | "underline" | "strikethrough" | "obfuscated"> = {
  k: "obfuscated",
  l: "bold",
  m: "strikethrough",
  n: "underline",
  o: "italic",
};

/** 一段带格式的文本片段（§ 码作用到下一个码为止）。 */
export type McFormatSegment = {
  text: string;
  color?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  obfuscated?: boolean;
};

/** 解析 MC § 码文本（语义同 Minecraft Java 版）：
 * - 颜色码 §0-§9/§a-§f（大小写不敏感）设置颜色，并**顺带清掉**粗体/斜体等格式；
 * - §r 重置颜色与全部格式；
 * - §l/§o/§n/§m/§k 叠加格式（保留当前颜色）；
 * - 未知代码（如 §z）与孤立 § 按字面保留。 */
export function parseMinecraftFormatting(text: string): McFormatSegment[] {
  const segments: McFormatSegment[] = [];
  let current: McFormatSegment = { text: "" };
  const push = () => {
    if (current.text) segments.push(current);
  };

  for (let index = 0; index < text.length; index++) {
    const char = text[index];
    if (char === "§" && index + 1 < text.length) {
      const code = text[index + 1].toLowerCase();
      const color = MC_COLOR_BY_CODE[code];
      if (color) {
        push();
        current = { text: "", color };
        index++;
        continue;
      }
      if (code === "r") {
        push();
        current = { text: "" };
        index++;
        continue;
      }
      const flag = MC_FORMAT_BY_CODE[code];
      if (flag) {
        push();
        const next: McFormatSegment = { ...current, text: "" };
        if (flag === "bold") next.bold = true;
        else if (flag === "italic") next.italic = true;
        else if (flag === "underline") next.underline = true;
        else if (flag === "strikethrough") next.strikethrough = true;
        else next.obfuscated = true;
        current = next;
        index++;
        continue;
      }
    }
    current.text += char;
  }
  push();
  return segments;
}

/** 去掉全部 § 码只留文字（原生 <option> / title 等不能渲染样式的场合用）。 */
export function stripMinecraftFormatting(text: string): string {
  return parseMinecraftFormatting(text)
    .map((segment) => segment.text)
    .join("");
}

/** 标签去掉 § 码后是否还有可显示的文字（纯格式码的标签按"没有标签"处理）。 */
function labelHasText(label: string | null | undefined): boolean {
  return Boolean(label && stripMinecraftFormatting(label).trim());
}

/** 参数显示名：lang 标签（option.<name>）优先，回退原始参数名。 */
export function optionDisplayName(option: ShaderpackOptionInfo): string {
  if (labelHasText(option.label)) return option.label as string;
  return option.name;
}

/** 取值显示名：lang 标签（value.<name>.<value> / 通用 value.<value>）优先，回退原值。 */
export function valueDisplayLabel(option: ShaderpackOptionInfo, value: string): string {
  const label = option.valueLabels.find((entry) => entry.value === value)?.label;
  if (labelHasText(label)) return label as string;
  return value;
}

/** 常见光影包 lang 文件的语言显示名（键 = 文件名字干小写）。 */
const LANG_DISPLAY_NAMES: Record<string, string> = {
  zh_cn: "中文",
  zh_tw: "繁體中文",
  en_us: "English",
  en_gb: "English (UK)",
  ja_jp: "日本語",
  ko_kr: "한국어",
  ru_ru: "Русский",
  fr_fr: "Français",
  de_de: "Deutsch",
  es_es: "Español",
  it_it: "Italiano",
  pl_pl: "Polski",
  pt_br: "Português",
  cs_cz: "Čeština",
  uk_ua: "Українська",
  tr_tr: "Türkçe",
};

/** 语言切换按钮的显示名：认得的 lang 干给母语名，认不出就原样显示字干。 */
export function langDisplayName(stem: string): string {
  return LANG_DISPLAY_NAMES[stem.toLowerCase()] ?? stem;
}
