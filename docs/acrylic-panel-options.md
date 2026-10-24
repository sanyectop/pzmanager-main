# 启动页亚克力面板：现状与备选实现方案

> 相关代码：`src/App.css`（`.pz-launch-account`、`.pz-launch-command`、`.pz-launch-icon-button`、
> `.pz-launch-workshop-info-*`、`.pz-launch-*-popover`）、`src/App.tsx`（`launchContent`）、
> `src/SeasonalWallpaper.tsx`（默认壁纸）。

## 1. 为什么删掉 `backdrop-filter`

启动页原先有 6 处 `backdrop-filter` 声明（5 条规则）：

| 规则 | 原值 | 说明 |
| --- | --- | --- |
| `.pz-launch-account` | `blur(28px) saturate(160%)` | Steam 账户面板 |
| `.pz-launch-command-toolbar .pz-launch-icon-button` | `blur(16px) saturate(160%)` | 设置 / 详情两个图标按钮 |
| `.pz-launch-settings-popover`, `.pz-launch-details-popover` | `blur(18px)` | 两个上弹面板（底色 0.96，本来几乎不透明） |
| `.pz-launch-workshop-info-toggle` | `blur(14px) saturate(130%)` | 创意工坊胶囊的开关 |
| `.pz-launch-workshop-info-details` | `blur(14px) saturate(130%)` | 创意工坊胶囊展开的内容 |

在 WebView2（Chromium）里 `backdrop-filter` 已知会产生这些显示问题，本项目里都能对上：

1. **圆角边缘光晕 / 渗漏**：模糊采样以元素矩形为准，圆角外侧仍被采到，浅色壁纸上出现一圈亮边。
2. **壁纸切换时拖影**：`.pz-app-shell` 的 `background-image` 是硬切（`transition: background-image` 无效，
   见 UI-23），backdrop 采样会晚一帧，出现闪白 / 旧底残留。
3. **四季壁纸每秒更新**：默认壁纸是 `SeasonalWallpaper`，每秒改一次叠图权重和光照层，
   backdrop 每帧都要重算，低端核显直接掉帧（UI_AUDIT_REPORT UI-20）。
4. **过渡期间模糊丢失**：整页 opacity/transform 动画会让 Chromium 丢掉面板的 backdrop-filter，
   代码里已经为此关掉启动页入场动画（`App.tsx` `navigateToPage`），属于"绕着 bug 写功能"。
5. **`position: fixed` 包含块**：每个 backdrop-filter 都成为固定定位的包含块，弹层必须走 Portal
   （`App.css:2101`、`App.tsx:1613/6049` 都留了补丁注释）。
6. **遮罩 / 滤镜祖先会截断采样（根因）**：`.pz-app-content` 上有滚动渐隐的 `mask-image`。
   带 `mask`（或 `filter` / `opacity` < 1 / `clip-path` / `backdrop-filter`）的祖先会成为
   **backdrop root**，它内部元素的 `backdrop-filter` 只能采到自己子树里的内容——背后的壁纸
   根本采不到，表现就是"写了模糊但一点效果都没有"。启动页里的所有 `backdrop-filter`
   当年都踩在这条上，这也是"有显示问题"最直接的解释。
   修复：`.pz-app-content-launch { mask-image: none; }`（启动页不滚动，渐隐本来也没有意义）。

结论：删掉之后启动页不再有任何 `backdrop-filter`，以上 6 类问题一起消失。

> 注意：`docs/promo/launcher-demo.html`（宣传页的启动页示意）仍保留了 `backdrop-filter`。
> 那是独立静态演示、不随应用发布，改它不影响运行；如果要让宣传图和实际观感一致，需要同步。

## 2. 现状：无模糊的"材质亚克力"

删掉模糊后，面板的"厚度"只由三件事撑起来：

- 半透明底色：`--pz-surface-acrylic-light` = `rgba(255,255,255,.6)`、`--pz-surface-acrylic-dark` = `rgba(32,20,28,.5)`；
- `::after` 的 `feTurbulence` 颗粒层（`opacity: .14`，`z-index: -1`）；
- 1px 玻璃描边 + 阴影 + 内高光。

注意点（已经处理）：`.pz-launch-account` 原来的层叠上下文是 `backdrop-filter` 顺带建的，
删掉后必须补 `isolation: isolate`，否则颗粒层的 `z-index: -1` 会沉到面板底色后面。

**还没处理的一条**：底色 0.6 / 0.5 是按"背后已经被模糊过"设计的。没有模糊之后，壁纸的高频纹理
会直接透上来，深色面板上的浅色文字在亮壁纸上对比度会掉。要保留现在的观感，选下文的任一方案时
都必须重新定这两个值（建议浅色 0.68–0.74、深色 0.6–0.66），或者把文字侧改成 `--pz-text-*` 的
深/浅自适应。

## 3. 方案对比

| # | 方案 | 默认四季壁纸也成立 | 观感 | 成本 | 风险 |
| --- | --- | --- | --- | --- | --- |
| A | 材质回退（纯 tint + 颗粒 + 描边，即现状调优） | ✅ | 半透明色块，看不到壁纸结构 | 极低 | 无 |
| B | 预模糊壁纸贴图（canvas 生成一张模糊图当面板底图） | ❌ 静态图可以，四季壁纸不匹配 | 像磨砂玻璃，但"模糊的不是背后那块" | 低 | 低 |
| C | **克隆壁纸层 + 面板形状裁剪 + 整体 blur**（手工 backdrop）**— 已采用** | ✅ | 与真亚克力几乎一致 | 中 | 中 |
| D | 面板内复制壁纸 + 逐像素对齐（`background-attachment: fixed` / JS 对齐） | ❌ | ≈ B | 中 | 高（fixed 附件与 filter 冲突） |
| E | WebGL/WebGPU 着色器磨砂（可带折射、颗粒） | ✅（要自己重绘壁纸） | 上限最高 | 高 | 中高 |
| F | 修 `backdrop-filter` 本身的问题（保留原实现） | ✅ | 原样 | 低–中 | 中（问题可能修不掉） |

## 4. 各方案细节

### A. 材质回退（当前基线；底色/光泽待调优）

Fluent 在"关闭透明效果"时用的就是这套：不模糊，用底色 + 噪点 + 高光模拟材料。

- 底色提亮/提深到能压住壁纸纹理（0.6→0.72 / 0.5→0.64 起步，用设置页的字体对比度滑块兜底）；
- 加一层斜向光泽：`background-image: linear-gradient(140deg, rgba(255,255,255,.14), transparent 45%)`；
- 顶边内高光 `inset 0 1px 0 rgba(255,255,255,.45)`，底边内暗线 `inset 0 -1px 0 rgba(0,0,0,.06)`；
- 颗粒层从 120px 换到 180px tile、`opacity` 0.14→0.10，避免在纯色上显成"脏点"。

优点：零成本、零风险、任何壁纸（含四季）都成立、彻底摆脱上面 5 类 bug。
缺点：没有"透过玻璃看"的感觉，本质是半透明色块。

### B. 预模糊壁纸贴图

- 壁纸应用时生成一次模糊底座（前端 canvas：`ctx.filter = "blur(12px) saturate(1.35)"`，
  输出 1/4 分辨率 WebP），挂到 `.pz-app-shell` 的 `--pz-wallpaper-blurred`；
- 面板：`background-image: var(--pz-wallpaper-blurred), linear-gradient(tint)`，
  `background-size: cover; background-position: center`；
- 因为图已经模糊，**不需要和壁纸逐像素对齐**，这是它比 D 便宜的全部原因；
- 面板底色必须提到 0.8 左右，否则下面那张**清晰**壁纸会透上来形成"双底"重影。

坑：默认壁纸是四季动态壁纸（`wallpaperSeasonal = !backgroundPath || builtin:seasonal`），
它的画面由 DOM 叠图 + SVG filter 每秒重绘，没有"一张图"可以预模糊；这种情况下必须回退到 A。

### C. 克隆壁纸层 + 面板形状裁剪（推荐，若要做真亚克力）

思路：不用 backdrop-filter，自己造一个"背景副本"，只让它出现在面板区域内。

```
.pz-acrylic-glass            /* position:absolute; inset:0; pointer-events:none; z-index: 1 */
  ├─ .pz-acrylic-glass-source    /* 壁纸的完整副本（<img> 或再渲染一次 <SeasonalWallpaper>) */
  └─ (noise / sheen 层，可选)
```

- 容器 `filter: blur(22px) saturate(1.4)`（blur 作用在副本上，一次栅格化）；
- 容器 `clip-path` = 所有亚克力面板的圆角矩形并集（`path()` 由 JS 生成，或 `<svg><clipPath>` +
  `<rect rx>`，几何由 `ResizeObserver` 同步：账户面板宽度随账号名变、启动面板宽度随内容变）；
- 面板本身改成 `background: transparent`（或很淡的 tint）+ 颗粒 + 描边 + 阴影，压在裁剪层之上。

为什么成立：模糊层是**不透明**的，直接盖住下面那块清晰壁纸，所以面板区域里就是模糊后的壁纸，
不需要反向挖洞。
为什么能覆盖四季壁纸：副本就是再渲染一次 `SeasonalWallpaper`（需要给 SVG filter id 加实例后缀，
现在写死成 `pz-seasonal-curve`），每秒跟随更新，blur 结果随之重算一次（1 Hz，可接受）。
代价：壁纸绘制量翻倍；两处几何同步代码；`clip-path` 变化会触发整层重绘。

变体 C'（更省）：不克隆整页，只克隆"面板包围盒 + 24px 外扩"那几块。收益是栅格化面积小，
代价是每块都要自己对位（等价于 D 的对齐问题），一般不值得。

### D. 面板内复制壁纸 + 逐像素对齐

`::before` 放同一张壁纸并试图用 `background-attachment: fixed` 与视口对齐——**这条不成立**：
一旦元素（或其祖先）有 `filter` / `transform`，fixed 附件就退化成随元素滚动的行为，而且
Chromium 在带 `filter` 的元素上做 fixed 附件时长期有 off-by-pixel 和错位问题。
只能用 JS 把 `getBoundingClientRect()` 反算成 `background-position / background-size` 写进样式；
既然要靠 JS 对齐，就不如直接做 C。

### E. WebGL / WebGPU 着色器磨砂

把壁纸（或四季合成结果）当纹理上传，着色器里做双边模糊 + 噪点 + 边缘折射 + 饱和度提升，
面板形状用 uniforms 传给全屏 quad 的 fragment shader。

- 上限最高：能做出真正的"折射边缘"和"液态玻璃"；
- 代价：需要 WebGL 上下文（WebView2 在部分机器上会掉到 SwiftShader）、
  四季壁纸要每秒重绘纹理并重新模糊、fallback 逻辑（无 WebGL → 回落 A）；
- 两个面板不值得。除非将来真要做"液态玻璃"全局材质，否则不选。

### F. 修 backdrop-filter 本身

如果"显示问题"其实只是其中一两类，这条最便宜：

- 圆角光晕 → 面板外包一层 `overflow: hidden; border-radius: inherit` 的壳（模糊裁在壳里）；
- 壁纸硬切 → 壁纸改成两层 `opacity` 交叉淡入（`transition: background-image` 本来就不生效）；
- 四季壁纸掉帧 → 把每秒整层重绘改成"权重变化 > 阈值才重绘"，面板加 `will-change`；
- 过渡丢模糊 → 已经在 `navigateToPage` 里绕过。

风险：如果根因是驱动/合成器层面的（老核显、远程桌面、软件渲染），上面这些都只是缓解。

## 5. 已落地的实现（方案 C）

代码：[`src/AcrylicGlass.tsx`](../src/AcrylicGlass.tsx)、`src/App.css` 的 `.pz-acrylic-glass*`、
`src/SeasonalWallpaper.tsx` 的 `idSuffix`。

结构（放在 `.pz-page-transition` 里、面板之前，随页面一起做过渡）：

```
.pz-acrylic-glass                 position:absolute; inset:0; z-index:0; pointer-events:none;
  │                               clip-path 兜底 inset(100% 0 0 100%)（= 什么都不画）
  └─ .pz-acrylic-glass-copy       inset:-32px; background-size:cover; background-position:center;
                                 filter: blur(var(--pz-acrylic-blur)) saturate(var(--pz-acrylic-saturate))
       └─ <SeasonalWallpaper idSuffix="-glass">   四季模式下壁纸的完整副本（静态图则走 background-image）
```

面板侧只加一个 `data-pz-acrylic="account|command|workshop"`，其余（半透明底色、颗粒层、描边、
阴影）完全不动 —— 所以配色和原来 `backdrop-filter` 版本逐层对应：

| 层 | 原来 | 现在 |
| --- | --- | --- |
| 背景模糊 | `backdrop-filter` 采样屏幕 | `.pz-acrylic-glass-copy` 的 `filter: blur()` |
| 底色 | `--pz-surface-acrylic-*` | 不变 |
| 颗粒 | `::after` feTurbulence | 不变 |
| 描边 / 阴影 | 规则不变 | 不变 |

几何：`AcrylicGlass` 挂载时量一次，之后 `ResizeObserver`（面板自身尺寸变化）+ `window.resize` +
每次组件渲染后各补一次，把各面板的**圆角矩形子路径拼成一个 `path()`** 写进 `style.clipPath`。
多个子路径靠 nonzero 填充规则天然取并集，面板重叠也不会互相挖洞。

数值：

- `--pz-acrylic-blur: 20px`、`--pz-acrylic-saturate: 1.45`（原实现是 28px/160%、14–18px 不等，这里统一一档）；
- 副本 `inset: -32px`：blur 在元素边缘会向透明区采样，不放大就会让贴近窗口边缘的面板发暗；
  副本内容本身是模糊的，这点缩放（约 3%）看不出来；
- 面板底色保持 0.6 / 0.5 不变。想更"透"可以只调这两个令牌（0.42 左右就能明显看出背景结构），
  代价是深色面板上的白字对比度下降。

### 验证方式（无 Tauri 的 Web 预览）

用 `vite build` 的产物 + 一段伪造 `window.__TAURI_INTERNALS__` 的脚本在 Edge headless 里跑起来截图，
逐项确认：

- ✅ 裁剪几何与面板严格重合（`getComputedStyle(glass).clipPath` 的坐标 = 面板 `getBoundingClientRect` − 亚克力层矩形）；
- ✅ 面板内确实是**模糊后**的壁纸：把 panel 底色临时去掉、与 `.pz-acrylic-glass { display:none }` 对比，
  树叶/栅栏等细节被抹平；
- ✅ 窗口 `resize` 后子路径数量与坐标同步更新（多挂一块 `data-pz-acrylic` 元素 → 从 2 条变 3 条）；
- ✅ 面板宽度变化后几何收敛（headless 里 ResizeObserver 通知很少，靠"每次渲染后补量"在一秒内收敛）。

注意：headless 下 `requestAnimationFrame` 几乎不跑（实测 1.8 s 内只有 2 帧），所以最初把重算
排进 rAF 的写法在该环境里永远不会执行；现在改成同步测量 + 渲染后兜底，不依赖帧调度。

## 6. 结论与后续

1. **已落地**：启动页 0 处 `backdrop-filter`，亚克力由"克隆壁纸层 + 面板形状裁剪"实现（方案 C）。
   所有壁纸模式（自定义图 / 创意工坊图 / 默认四季动态壁纸 / 纯色画布）都成立。
2. **成本**：四季模式下壁纸会被渲染两份（真壁纸 + 亚克力副本），模糊是整层一次性栅格化，
   壁纸每秒更新时随之一秒一次。低端机器如果掉帧，先降 `--pz-acrylic-blur`，再考虑
   "只在静态壁纸下启用亚克力"。
3. **不建议回头用**：B（只适合静态壁纸且是"假模糊"）、D（`filter` 会让 `background-attachment: fixed` 失效）、
   E（成本与收益不匹配）、F（除非能把原问题定位到单一可修点）。
4. **浮层另算**：菜单 / 下拉 / 右键菜单 / 启动页上弹面板仍然用 `backdrop-filter`
   （见 `docs/ui-style-guide.md` §2.3）。它们是短暂存在的小面积层、不参与页面过渡，第 1 节列的
   那几类问题不适用；而且它们会叠在卡片、列表等任意内容上，"克隆壁纸"会把内容换掉，不可行。
5. 待办（可选）：低端设备 / `prefers-reduced-motion` 下的"关闭亚克力"开关，直接切回方案 A 的纯材质。
