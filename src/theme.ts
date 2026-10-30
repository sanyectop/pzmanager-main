import { createSystem, defaultConfig, defineConfig, defineRecipe } from "@chakra-ui/react";

/**
 * 全局按钮的亚克力底。
 *
 * 只改 `ghost` / `outline` 两种原本透明的变体：半透明浅色底 + 背景模糊 + 细描边 +
 * 顶边内高光，参数复用菜单 / 弹窗那套令牌，所以「设置 › 通用 › 模糊度」仍然全局生效。
 * `solid`（主色按钮）保持实色，不参与亚克力——否则主操作会被洗白、失去强调。
 */
const acrylicButton = defineRecipe({
  variants: {
    variant: {
      ghost: {
        bg: "var(--pz-btn-tint)",
        borderColor: "var(--pz-line)",
        boxShadow: "inset 0 1px 0 var(--pz-btn-highlight)",
        backdropFilter: "blur(var(--pz-menu-blur)) saturate(var(--pz-menu-saturate))",
        _hover: { bg: "var(--pz-btn-tint-hover)" },
        _expanded: { bg: "var(--pz-btn-tint-hover)" },
      },
      outline: {
        bg: "var(--pz-btn-tint)",
        borderColor: "var(--pz-line-strong)",
        boxShadow: "inset 0 1px 0 var(--pz-btn-highlight)",
        backdropFilter: "blur(var(--pz-menu-blur)) saturate(var(--pz-menu-saturate))",
        _hover: {
          bg: "var(--pz-btn-tint-hover)",
          borderColor: "var(--pz-line-emphasis)",
        },
        _expanded: { bg: "var(--pz-btn-tint-hover)" },
      },
    },
  },
});

export const system = createSystem(
  defaultConfig,
  defineConfig({
    theme: {
      recipes: {
        button: acrylicButton,
      },
    },
  }),
);
