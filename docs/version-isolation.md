# 游戏版本隔离（方案 D：母文件 + 增量链）

版本页在文件系统层面保存 PZ 游戏目录的历史版本，与 Steam 的更新机制解耦。
入口是顶部导航的「游戏管理」（正方体图标，`src/app/constants.ts` 的 `navigationItems`），不再是设置里的子分类。
首个版本是完整快照（母文件），之后每次捕获只保存变化文件的内容定义分块增量；
任意历史版本都能从最近的完整快照开始，按父链应用增量重建。

页面左侧分为六个分区（`src/App.tsx` 的 `versionSections`）：「概览」承载保存/设置与当前状态，
「模组版本」按模组隔离，「历史版本」只展示游戏版本链（改动统计、还原、删除，以及还原时的
后台进度），另有「支持库」「光影包」「截图」三个分区。本文档只聚焦版本隔离本身，
支持库 / 光影包 / 截图不在展开范围（旧的独立入口已重定向进版本页对应分区）。

## 存储布局

存储目录默认是 `<用户 Zomboid>/.pzmanager/versions`，可在页面里修改。目录**不能**
放在游戏安装目录内（启动器会拒绝），否则快照会把自己也扫进去。

```text
versions/
├── base/<id>/                第一个完整快照（母文件）
├── checkpoints/<id>/         后续完整快照（检查点）
├── deltas/<from>__<to>/      每次捕获的增量包与 changelog.json
├── manifests/<id>.json       每个版本的文件清单
├── trash/restore-*/          还原时移出的、目标版本里不存在的文件
├── tmp/                      操作期间的临时文件
└── meta.json                 版本链、Steam buildid 与活动版本
```

每个版本同时写入 `manifests/<id>.json` 和 `meta.json` 的记录：

* `rootHash`：对所有文件的相对路径（小写）、大小、SHA-256 按稳定顺序再哈希。
* `metadataHash`：只含相对路径 + 大小，用于打开页面时快速识别当前目录，不读全量内容。
* `steamBuildId` / `branch`：来自 Steam 库里的 `appmanifest_108600.acf`。
* `changeLog`：相对上一版本的新增、删除、修改文件（增量版本才有）。

## 增量格式（PZD1）

不依赖外部 `xdelta3` 可执行文件，增量引擎内置在 `src-tauri/src/versioning.rs`：

* 基准文件和目标文件都按 Gear hash 做内容定义分块（CDC，16 KiB–128 KiB）。
* 目标分块如果能在基准索引里找到相同 SHA-256 和长度，写 `COPY(offset, length)`；
  否则写 `ADD(length, bytes)`。
* 插入/删除一段数据只影响附近分块，后续分块能重新对齐，不会整体偏移。
* 头部记录基准/目标长度与哈希；应用后先校验增量 trailer，再校验清单里的 SHA-256。
* 新增文件用空基准生成增量（内容会完整写入，但这是新增文件无法避免的成本）。

## 操作流程

1. **创建基准版本 / 检查点**：扫描并哈希整个游戏目录，完整复制到 `base/` 或
   `checkpoints/`，写入清单。最新版本已经是同内容完整快照时会跳过重复复制。
2. **捕获新版本（增量）**：遍历当前目录，与最新版本清单比对；**大小和修改时间（毫秒）
   都没变的文件直接沿用上次的 SHA-256，不打开文件**（只记录变化时主要省在这里，
   也避开杀毒软件对每个被打开文件的实时扫描）；其余文件重新哈希，只对新增/修改文件
   生成增量包，删除文件只记路径。随后写清单、changelog，追加到版本链。
3. **校验当前目录**：重新哈希所有文件，用根哈希确认当前目录对应哪个版本。
4. **还原到此版本**：先完整校验当前目录是否属于某个已记录版本（不属于时要求用户
   明确确认）；再逐个文件从最近完整快照 + 增量链重建，未变化的文件保留，目标版本
   没有的文件移到 `trash/restore-*`。每个写出的文件都校验 SHA-256。
5. **转换为独立版本**：把某个增量版本与最近一次完整快照重新比对，重建它的增量包并把
   父版本改成该完整快照。转换后它不再依赖中间的旧版本，链路深度变成 1 层，可以继续
   删除它之前的版本。
6. **删除版本**：任意版本都可以删除。如果它后面还有增量版本依赖它，删除前会先把这些
   版本自动转换为独立版本（phase = `rebase`），再删文件；基础版本（根完整快照）仍被
   后面的版本依赖时不允许删除（界面禁用该按钮并说明原因）。
7. **删除该版本及更早**：一次删掉目标版本和它之前的所有版本。删不掉依赖链，所以先把
   后面的幸存版本**物化成新的完整基准**（增量版本逐文件重建到 `base/`，完整备份挪到
   `base/`，phase = `copying`），再从新到旧删除；被降级版本的旧增量包随后清掉。
8. **还原前预检**：前端在弹确认框之前先查游戏是否在运行（`is_project_zomboid_running`），
   开着就直接提示，不再让用户读完确认文案才失败。

版本页面会读取 `appmanifest_108600.acf` 的 `buildid`，与最后捕获的 buildid 比较，
变化时给出「Steam 可能已经更新」提示。还原旧版本不会改变 Steam 的更新状态。

## 多线程与单线程

- 版本页 ›「多线程扫描与复制」默认开启；关闭后扫描、快照复制和还原都回到原来的单线程路径。
- 并行度默认 `min(逻辑核心数, 16)`。哈希用 `std::thread::scope` + work-stealing 文件队列；
  完整快照复制用 **rayon 有界线程池**（`ThreadPoolBuilder::num_threads(workers)` +
  `par_iter().for_each_init`），每个线程复用一份 1 MiB 复制缓冲，避免逐文件分配。
- 复制是流式的：大文件每读一块都检查取消，能在块边界中断（不再等整个文件拷完），
  结束按清单里的 size 校验；进度由调用线程每 120ms 读原子计数上报，不会跨线程碰 `progress`。
- 并行范围：`scan_directory` 的 SHA-256 计算、完整快照复制、还原时目标文件的物化与写入。
- 扫描的元数据只遍历一次：`collect_walk_entries` 用 `DirEntry::metadata()` 顺手取大小和 mtime
  （Windows 的目录枚举自带这些字段，不再逐个 `symlink_metadata`），哈希阶段直接用这份结果；
  每个 worker 复用同一个 1 MiB 读缓冲，不再每文件 `vec![0u8; 1 MiB]`。
- 捕获阶段的增量包生成仍按文件串行（每个文件的 CDC 索引内存不小）；扫描阶段本身已并行。
- 进度只由主线程回调，worker 通过通道回传结果；取消在文件边界生效，还原写入阶段不中断。
- 机械硬盘上并行随机读写可能更慢，建议关闭开关改用单线程。
- 增量捕获的缓存只信任「大小 + 修改时间」：如果某个工具改了文件内容却把这两个值
  原样保留，这次改动会被漏掉。Steam 更新、正常复制/编辑都会刷新 mtime，不受影响；
  需要强制全量重算时，用「检查当前版本」或重新做一次完整保存。

## 与 PZ Optimization 协同

PZ Optimization 通过 `install.ps1` 替换游戏类文件；切换游戏版本后，这些文件可能被覆盖，
模组状态也会失效。还原流程会自动处理：

1. 还原前检测 PZ Optimization 是否已安装；已安装则调用它的卸载脚本恢复原版文件，
   卸载失败会中止还原，避免在补丁状态上叠加还原。
2. 还原成功后，如果之前已安装，会自动重新安装匹配“还原后游戏版本”的 Workshop 构建。
   安装器路径在 `steamapps/workshop/content/108600/3805285544/mods/PZ_Optimization/<版本>/install.ps1`
   下动态查找，不依赖固定的 `42` 目录。
3. 自动重装失败不会回滚已经完成的还原，只在结果里给出警告；可以到「支持库」手动安装。
4. 如果还原前未安装 PZ Optimization，流程不会安装它；想保留原版状态，请先到「支持库」卸载。
5. 如果当前游戏 revision 在 Workshop 里没有匹配的 `pzopt-classes`，但检测到官方 `install.ps1`，
   「支持库」会显示「强制安装（官方脚本）」。启动器把脚本复制到临时目录后运行，让脚本按当前
   revision 从 GitHub 下载匹配构建；不会使用旁边的旧类文件硬装。如果作者尚未发布该版本，
   脚本会报 `no release for game revision ...`，这是 PZ Optimization 上游还没出对应构建，不是启动器故障。

## 前端命令

| 命令 | 说明 |
| --- | --- |
| `get_version_status` | 读取版本链、Steam buildid、当前目录的元数据匹配结果 |
| `save_version_store_path` | 保存版本存储目录（空 = 默认目录） |
| `save_version_parallel` | 保存多线程扫描/复制开关 |
| `start_version_snapshot` | 后台创建完整快照，进度走 `version-progress` 事件 |
| `start_version_capture` | 后台捕获增量版本 |
| `start_version_restore` | 后台还原到指定版本 |
| `start_version_verify` | 后台做根哈希校验，额外推送 `version-verified` |
| `start_version_rebase` | 后台把指定增量版本转换为独立版本（重写增量包，phase = `rebase`） |
| `cancel_version_operation` | 请求取消（还原开始写文件后不再中断，避免半还原） |
| `delete_version` | 删除任意版本；需要时先自动重挂后面的版本，再删除 |
| `delete_version_through` | 删除该版本及更早；后面的版本先重建成新的完整基准 |

扫描/复制/差分都在后台线程执行；同一时间只允许一个版本操作。开启多线程后，
单个操作内部再按文件并行。

进度事件（`version-progress` / `mod-version-progress`）除百分比外还带 `step` / `steps`：
一次操作被拆成「计算文件哈希 → 复制/保存变化 → 保存模组版本」这样的步骤，前端显示成
`10% 1/3`。映射表在 `lib.rs::version_step_for`：

| 操作 | 步骤 |
| --- | --- |
| 游戏完整保存 / 保存变化 | 1 计算哈希 → 2 复制文件 / 保存变化 → 3 保存模组版本（仅当开启「保存时同时备份模组」且有启用模组） |
| 游戏还原 | 1 检查当前文件 → 2 还原文件（含 PZ Optimization 卸载/重装）→ 3 清理多余文件 → 4 还原关联模组版本（有大版本关联时） |
| 模组完整保存 / 保存变化 | 1 计算哈希 → 2 复制文件 / 保存变化 |
| 模组还原 | 1 检查 → 2 还原 → 3 清理 |
| 转换独立版本 | 1 步（重写增量包，phase = `rebase`），前端不显示计数 |
| 删除版本 | 1 步；删除旧版本时先重挂后续版本（phase = `rebase`），再删文件（phase = `deleting`） |
| 删除该版本及更早 | 1 步；先物化新基准（phase = `copying`），再逐个删除（phase = `deleting`） |
| 校验 | 1 步，前端不显示计数 |

每一步的百分比各自从 0 走到 100。模组步骤按**模组**等分：处理第 i 个模组时是 `(i-1)/N` 到 `i/N`，
模组内部的扫描/复制只在这一小格里平滑插值（phase 统一写成 `mods`），所以进度条不会因为
单个模组里的文件数忽高忽低，也不会退回第 1 步。

## 当前实现边界

* 检查点是手动创建的；页面在增量链达到 5 层时会提示创建检查点。
* 还原是逐文件写入的；中途遇到 I/O 错误会中止，已写入的文件保留，修复后可重试。
* 还原时移出的文件放在 `trash/restore-*`，不会自动清理；确认不再需要后可以手动删除。
* 变更清单是文件级；块级明细没有在界面上展开（增量包本身是块级的）。
* 不采用符号链接挂载活动视图：还原直接覆盖游戏目录，目标版本没有的文件进回收区。
* `mtime` 只记录在清单里，不参与版本识别；识别先用路径 + 大小，校验时才读全量哈希。

## 设备掉线（os error 433 / 1167）

完整保存要在几秒内读完整个游戏目录（带模组库时接近 10 GB）。如果硬盘在这期间从
系统里掉线，Windows 会让**整块盘**上的所有路径都返回 `ERROR_NO_SUCH_DEVICE (433)`
或 `ERROR_DEVICE_NOT_CONNECTED (1167)`，资源管理器里同样显示「指定不存在的设备」，
重启电脑后设备重新枚举才恢复。这不是文件损坏，也不是版本库的问题：版本库默认在
C 盘（`<用户 Zomboid>/.pzmanager/versions`），保存过程对游戏目录只读。

软件侧的应对：

* 错误信息把「磁盘 X: 已经从系统断开」放在最前面，不再伪装成某个文件读取失败；
* 一键保存模组 / 整合包还原遇到这类错误立即中止，不会把剩下几百个模组逐个撞一遍；
* 不自动重试设备级错误——盘没回来之前重试只会重复失败，还会刷满日志。

排查方向：换数据线 / 换 USB 口或主板接口、检查供电；用 CrystalDiskInfo 看 SMART，
或在「事件查看器 → Windows 日志 → 系统」里找 `disk` / `storahci` / `nvme` 来源的
事件（7、11、51、153 等）。

## 测试

```powershell
cd src-tauri; cargo test --lib versioning::
node --test tests/versioningUi.test.ts tests/versionParallel.test.ts
```

Rust 测试覆盖：根哈希稳定性、CDC 增量往返、完整快照 → 增量 → 检查点 → 还原、
未知目录的强制还原门控、元数据指纹、并行扫描结果与串行一致、并行取消。
前端测试覆盖版本展示、进度计算，以及多线程开关的默认值/持久化契约。

---

# 模组版本隔离（复用同一套引擎）

模组版本隔离和游戏大版本共用 `versioning.rs` 的扫描 / CDC 增量 / 清单 / 还原 / 校验引擎，
区别只在「目标根目录」和「存储分区」：每个模组目录是一条独立版本链。

## 存储布局

版本存储目录（版本页 › 版本存储目录）下多出一个 `mods/` 分区：

```text
<store>/
├── base/ checkpoints/ deltas/ manifests/ trash/ tmp/ meta.json   ← 游戏大版本
└── mods/
    └── <key>/                                                    ← 每个模组一条链
        ├── base/<id>/ checkpoints/<id>/ deltas/<from>__<to>/
        ├── manifests/<id>.json  trash/  tmp/
        └── meta.json
```

`<key>` 由 `mod_versioning::store_key` 生成：

* 创意工坊模组：`workshop-<workshopId>-<modId>`；
* 本地模组：`local-<modId>`；
* `modId` 统一小写、只保留 `[a-z0-9._-]`，其余折成 `_` 并截断到 48 字符；
* 拿不到可用 ID（例如目录名是中文、`mod.info` 缺失）时退化为 `<source>-<路径 SHA-256 前 8 位>`。

模组 ID 稳定，Steam 库搬家后仍能找回原链；用路径哈希兜底的情况在目录改名后会变成「未记录」。

## 命令

| 命令 | 作用 |
| --- | --- |
| `list_mod_version_targets` | 列出所有物理模组根目录 + 各自版本数/最新记录（只读 `meta.json`，不扫描文件） |
| `get_mod_version_status` | 单个模组的完整状态：版本链、当前目录匹配情况、待捕获变化 |
| `start_mod_version_snapshot` | 为该模组创建完整快照（没有历史时用） |
| `start_mod_version_capture` | 记录增量版本 |
| `start_mod_version_restore` | 还原到指定版本（先要求游戏关闭） |
| `start_mod_version_verify` | 根哈希校验，额外推送 `mod-version-verified` |
| `start_mod_version_rebase` | 把该模组的指定增量版本转换为独立版本 |
| `delete_mod_version` | 删除该模组的任意版本；需要时先自动重挂后面的版本 |
| `delete_mod_version_through` | 删除该版本及更早；后面的版本先重建成新的完整基准 |
| `start_mod_version_save_changes` | 一键保存模组变更：逐模组元数据比对挑出有变化的（或还没有版本的），只保存它们；无变化自动跳过。保存成功后写一条「变更记录」（kind=changes 的整合包），可整套还原 |

进度走独立事件 `mod-version-progress`，payload 在游戏事件基础上多 `rootKey` / `rootPath`；
互斥仍共用 `VERSION_OPERATION_ACTIVE`，所以游戏大版本与模组版本操作不会并发。

## 一键保存模组变更 / 整合包（一键保存全部模组）

版本页 › 模组版本 的「一键保存模组变更」只保存**有待捕获变化**（或还没有任何版本）
的模组：先对每个模组做一次元数据比对（只看路径 + 大小，不哈希），无变化的模组
自动跳过。保存成功后写一条**变更记录**（`kind = "changes"` 的整合包，标签「变更 N」，
N 只按变更记录数编号），可以在整合包列表里「还原整套」——只还原这次保存的那几个
模组，完整快照仍由「一键保存全部模组」承担。全部无变化时不写记录、直接提示完成；
保存时不需要关闭游戏。

版本页 › 模组版本 的「一键保存全部模组」把**当前所有物理模组根目录**各记一个版本
（第一次是完整备份，之后是增量），再把「模组键 → 版本 ID」的引用表写成一条**整合包**。
它不是新的快照格式：每条模组版本仍在自己的链里，整合包只是某一时刻所有模组版本的集合。

- 记录文件：`<版本存储目录>/modpacks.json`（`format` + `packs[]`，camelCase），
  每条包含 `id`、`label`、`createdAt`、`kind`（`all`＝整合包 / `changes`＝变更记录；
  加 kind 之前的旧文件没有该字段，读回按 `all` 处理）、`mods[]`（`ModVersionRef`）。
- 整合包保存前会弹命名框：可以给整合包起名（去空格后最长 60 字），留空回落默认标签
  「整合包 N」；变更记录不弹命名框，自动叫「变更 N」（之后可用「重命名」改）。
  保存时都不需要关闭游戏（只读模组目录 + 写版本库）。
- 「重命名」只改 `modpacks.json` 里的显示名（`rename_mod_version_pack`），模组版本
  引用和还原范围不变；操作进行中会拒绝改名。
- 「还原整套」要求游戏已关闭：按 `key` 找当前模组目录（找不到再退回记录的路径），
  逐个 `restore_version(..., force = true)`，随后同步 `jar-mod-state.json`；已卸载的模组跳过。
- 「删除」只删这条整合包记录，各模组自己的版本历史保留。
- 进度走 `mod-version-progress`，`operation` 为 `modSaveAll` / `modRestorePack` / `modSaveChanges`，
  只有一步；任务中心会显示「整合包 · <名称>」或整合包标签本身。
- 注意：第一次保存会把**所有已安装模组**（包括未启用的）各复制一份，占用空间可能很大；
  只想保存当前在用的模组，用游戏版本的「保存时同时备份模组」即可。

命令：`start_mod_version_save_all`、`start_mod_version_restore_pack`、
`list_mod_version_packs`、`rename_mod_version_pack`、`delete_mod_version_pack`。

## 游戏大版本关联模组

* 版本页 › 游戏版本里「捕获时包含模组」默认开启（`version_include_mods`）。
* 创建/捕获游戏大版本成功后，对**当前已启用**的模组（`default.txt` 里的 Lua ID +
  `jar-mod-state.json` 里未禁用的 JAR 模组）逐个记录版本：没有历史时建基准，否则记增量；
  内容没变化时直接引用当前链头。进度阶段为 `mods`。
* 结果以 `ModVersionRef`（`key/modId/name/source/workshopId/versionId/rootPath`）写进该游戏版本的
  `VersionRecord.modVersions`；模组自己的链里该字段为空。
* 还原游戏大版本时，`restore_associated_mod_versions` 按 `key` 找当前模组目录（找不到再用记录路径），
  `force = true` 逐个还原并同步 `jar-mod-state.json`；模组已卸载的跳过并在结果里注明。
* 删除游戏版本不会删除模组链；两条链各自独立。

## 还原模组后的 `jar-mod-state.json`

模组目录里存在 `mod.info` ⇔ 启用。还原后 `reconcile_jar_state_for_root` 会把状态文件里的
`disabled` 对齐到磁盘实际状态，否则下次打开模组页时 `apply_jar_mod_file_state` 会把
`mod.info` 又改名回 `mod.info.disabled`，看起来像还原没生效。

## C 盘备份提醒

版本页的所有操作（游戏版本 / 单个模组 / 整合包）在开始前都会检查实际使用的版本
存储目录（`get_version_store_path`）。目录在 C 盘时弹出换盘提醒：

* 弹窗提供「去换盘」「仍要继续」两个出口，以及「不再弹出该提示」复选框。
* 勾选后写进应用设置 `version_system_drive_notice_dismissed`，之后不再拦截操作。
* 「备份保存位置」分区在 C 盘时仍保留一条常驻警告，并提供「重新开启换盘提示」。
* 只有 `C:` 开头（不区分大小写）算系统盘；其它盘符和 UNC 路径都不触发。

## 边界

* 只记录文件内容；启用顺序/开关仍由「模组预设」负责。
* 记录/校验是只读操作，可以在游戏运行时进行；还原要求游戏已关闭。
* 创意工坊目录可能被 Steam 校验或更新再次覆盖，这是「回滚被更新换掉的模组」的预期行为。
* 删除模组/退订不会删除对应版本库；暂不提供孤立版本库清理。
* 模组列表按物理目录去重（本地副本与工坊副本各有一条链），而模组管理页按模组 ID 合并显示。

## 测试

```powershell
cd src-tauri; cargo test --lib mod_versioning::
cd src-tauri; cargo test --lib versioning::tests::attaches_mod_version_references_to_a_game_version
node --test tests/modVersionUi.test.ts tests/versioningUi.test.ts
```
