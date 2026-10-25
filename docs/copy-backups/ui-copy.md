
## 2026-10-24 光影包下载页副标题(src/App.tsx,已应要求删除)

> 使用 SJMCL 同款 Minecraft 光影源，支持 CurseForge 与 Modrinth。

位置:`<Text color="gray.500" fontSize="sm" mt="1">` 于光影包下载面板头部,原在标题「光影包下载」之下。

## 2026-10-25 内置壁纸名称(src/App.tsx, builtinBackgrounds)

> 四季动态壁纸

位置:`builtinBackgrounds` 数组项的 `name` 字段(壁纸选择菜单项),因「动态」有歧义改为「四季壁纸」。
关联注释(App.tsx:497、AcrylicGlass.tsx:23)同步去掉「动态」。

## 2026-10-25 翻译回退源文案(src/App.tsx, 两处)

> 翻译自定义文本，与开发者新闻、创意工坊详情使用同一条翻译线路（Google 优先，失败自动换用 Yandex）。

位置:`App.tsx:6461` 翻译器设置页描述。Yandex 免费 API v1 已上游停服(403 Session invalid /
410 no longer active),回退源替换为 MyMemory,文案同步更新。

> yandex: "Yandex"

位置:`App.tsx:7993` 引擎标签映射,改为 mymemory: "MyMemory"(引擎缓存仅在内存中,无旧数据兼容问题)。

## 2026-10-25 关于页「项目仓库」占位文案与内置壁纸名称(src/App.tsx,仓库补链 + 壁纸改名前的快照)

> 项目仓库
>
> 尚未上传，敬请期待。

位置:`App.tsx` aboutSettingsContent 末尾。仓库已发布(GitHub:sanyectop/pzmanager-main),
占位文案替换为真实链接,并在其上方新增「官方网站」(https://pzm.sanyec.top) 一行。

> 四季壁纸

位置:`builtinBackgrounds` 数组项 `name` 字段与启动页时长标签,改名为「四季轮转」;
关联注释同步更新。
