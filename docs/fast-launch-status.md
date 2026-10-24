# 极速启动（实验性）——实现状态与交接文档

> 最后更新：2026-10-14 深夜（全链路已在一次性 JVM 上跑通；**游戏内实测仍未做**）
> 上游规格：`docs/architecture.md` §6.3「启动链与模组加载器方案」中"极速启动"相关段落
> 需求来源：《僵毁启动速度优化思路.md》（用户提供）

---

## 1. 目标（一句话）

把启动器已经做过的 JAR 预扫描结果做成缓存，在 **ZombieBuddy 自己扫描之前**喂给它的
`PatchEngine.collectPatches`，短路其中的 ClassGraph 全量扫描，从而把启动耗时的约 20 秒砍掉；
**任何一步失败都必须回退到 ZombieBuddy 原生扫描**，且必须能在设置里开关、只对
「模组加载器方案 = ZombieBuddy」生效。

---

## 2. 目前的状态（结论先行）

| 部分 | 状态 | 证据 |
| --- | --- | --- |
| 设置 › 启动 › 极速启动开关（仅 zombieBuddy 可用 + 免责声明 + 缓存状态行） | ✅ 完成 | 前端构建通过；状态行数据来自 `fast_launch_cache_status` |
| 预扫描缓存产出（只收「已批准且启用」） | ✅ 完成并**实测** | 一次真实扫描产出 **25~26 个模组 / ~45 KB / 16 位十六进制指纹**，含 `Viewpoint 92 patches / 8 exposers` |
| **缓存持久化（只在「刷新 JAR 列表」时重建）** | ✅ 完成 | `should_write_fast_launch_cache()`：显式刷新必重建、自动扫描仅在缓存缺失时补一份；`write_jar_mod_state()` / `set_jar_mod_info_enabled()` 里的自动删除已移除（有单测） |
| 缓存一致性状态（提示用，不再自动删） | ✅ 完成 | `fast_launch_cache_matches()` / `fast_launch_cache_status()`；设置页状态行在不一致时提示重建 |
| 短路点定位 | ✅ 已确认 | `PatchEngine.collectPatches`（22 处 classgraph 引用；`Loader`/`Agent`/`Patch` 均为 0） |
| 字节码工具链（读 class、包装式改写） | ✅ 完成并在真实类上验证 | 对真实 `PatchEngine.class` 逐字节断言"原实现不变、方法数 +1、包装体正确" |
| 辅助类 `FastLaunchBridge`（含真原生回退） | ✅ 完成 | Java 源码在仓库、已编译 7.4 KB、`include_bytes!` 嵌入 DLL |
| **把改写后的字节码送进 JVM** | ✅ **已打通** | `GetEnv` 卡点根因 = 函数表指针层级写错；修好后 `-agentpath` 全链路跑通，harness 实测 `collectPatches$pzm` 存在、缓存命中直接返回、未命中回退原生实现 |
| **游戏里真的生效** | ⚠️ **两轮实测都没加速，两个不同的根因都已修；最后一次复验被中断，尚未证明生效** | 第一轮：`PatchEngine` 在 hook 生效前就被 ZB 定义了（§6.2）；第二轮：hook 生效且 `PatchEngine` 确实被改写，但**缓存路径对每个模组都落空**（类名格式 bug，§6.3） |
| 启动器接线（`-agentpath:` 写进游戏启动文件，且必须最前） | ✅ 完成 | `launch_project_zomboid` → `apply_launch_loader(..., fast_launch_agent)`；关开关/换方案/DLL 不存在时自动摘掉；`-agentpath:` 排在 `vmArgs[0]`（有单测） |
| 游戏内实测（修完类名格式之后） | ❌ **未做**（上一次复验命令被中断） | 见 §10.1 复现步骤 |
| 文档同步 | ✅ 已同步 | `docs/architecture.md` §6.3（含**推翻**旧的"手写虚表会崩"结论）；本文档 |

**对玩家的实际影响**：开关打开、方案是 ZombieBuddy、且覆盖层 DLL 存在时才会写 `-agentpath`；
注入成功但缓存不可用/改写失败/辅助类缺失等任何一步不成立，都是"放行原字节" = ZombieBuddy 原生扫描。
关掉开关后会优先恢复启用前的启动文件；恢复失败才只摘掉 `-agentpath:`。见 §7「安全状态」。

---

## 3. 实现结构（代码在哪）

### 3.1 启动器侧（`src-tauri/src/lib.rs`）

* `FAST_LAUNCH_CACHE_FORMAT = 1`、`FAST_LAUNCH_CACHE_FILE = "zb-prescan-cache.json"`；
* `FastLaunchCache` / `FastLaunchCacheMod`（serde，`camelCase`：`format` / `generatedAt` / `modSetHash` / `mods[]`，每个模组含 `modName` / `jarPath` / `basePackage` / `jarHash` / `patchClasses` / `luaClasses`）；
* `build_fast_launch_cache(mods, generated_at)`：**只收 `!disabled && decision == Some(true)`**，按 jar 路径排序后算指纹；类名经 `binary_class_name()` 落成**二进制名（点号）**——扫描给的是内部名（斜杠），注入器要用 `Class.forName`，见 §6.3；
* `fast_launch_fingerprint()`：FNV-1a（不引哈希依赖）；
* `write_fast_launch_cache_at()` / `read_fast_launch_cache_at()` / `fast_launch_cache_matches()`；
* `fast_launch_enabled()`（读设置）、`fast_launch_cache_path()`、`should_write_fast_launch_cache(refresh_cache, cache_exists)`（**缓存持久化策略**：显式刷新才重建）、`invalidate_fast_launch_cache()`（保留给测试/将来"清空缓存"入口，**不再自动调用**）；
* 命令：`save_fast_launch(enabled)`、`fast_launch_cache_status()`、`start_zombie_buddy_jar_scan(refresh_cache)`；
* 扫描流程（`start_zombie_buddy_jar_scan`）在收集完结果后，**只有 `should_write_cache && fast_launch_enabled()` 才写缓存**（写失败只记日志、不打断扫描）；
* **`-agentpath:` 接线**：
  * `FAST_LAUNCH_AGENT_PREFIX = "-agentpath:"`、`is_fast_launch_agent_token()`、`normalize_fast_launch_agent_tokens()`（保证"有且只有一条、且必须排在最前"，`None` = 全摘掉）、`fast_launch_agent_for_batch()`（路径含空格时不写 `.bat`）；
  * `launch_project_zomboid(memory_mb, launch_args, fast_launch_dll_path)`：只有 `fastLaunch == true` **且** 方案是 `zombieBuddy` **且** `resolve_fast_launch_agent_path()` 确认 DLL 存在时才写；否则传 `None` 摘干净（`-agentpath` 指向的文件不存在会让 **JVM 直接启动失败**，绝不能留）；
  * `resolve_fast_launch_agent_path()` 复用 `overlay::imp::normalize_dll_path()`（绝对化 + 存在性 + 扩展名校验）；
  * 写入位置：`ProjectZomboid64.json` 的 `vmArgs[0]`（**必须在最前**，否则 hook 赶不上 ZB 载入 PatchEngine，见 §6.2），必要时也写 `ProjectZomboid64.bat` 的 `SET _JAVA_OPTIONS`；**不写 Steam LaunchOptions**（那是空白分隔的字符串，路径带空格会坏）；
  * 写入仍走原有备份 + 事务（`game_path/.pzmanager-backups/<时间戳>/`、`apply_file_transaction`）；
  * **打开开关先备份、关掉开关先恢复**：`save_fast_launch(true)` 在落盘前把 `ProjectZomboid64.json` / `.bat` 的干净版本备份到 `%USERPROFILE%\Zomboid\.pzmanager\fast-launch-backup\`（含 `manifest.json`）；`save_fast_launch(false)` 优先用这份备份整体恢复启用前的文件，恢复失败才退回 `apply_launch_loader(None, policy_agent, None)` 只摘 `-agentpath:`。恢复前会把当前文件另存到 `game_path/.pzmanager-backups/<时间戳>/`，不会丢现场；
  * 缓存位置：`%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`。

### 3.2 注入器侧（`overlay-dll/`）

* `src/classfile.rs`——**字节码工具链**（零依赖，纯 `std`）：
  * 常量池解析（`ClassFile::parse`，正确处理 `long`/`double` 占两槽）、方法表与 `Code` 属性定位（`parse_methods`）、布局定位（`layout`）；
  * 增量编码（`encode_utf8/class/name_and_type/methodref`、`encode_static_call_reference`）；
  * `encode_short_circuit_body()`：`aload_0 / aload_1 / invokestatic <辅助类> / areturn`（6 字节）；
  * **`wrap_collect_patches()`（核心）**：把原方法改名 `collectPatches$pzm`（只改它自己的 `name_index` 这两字节，ZombieBuddy 的调用点不受影响）＋追加一个同名包装方法（签名相同、体为短路字节码）＋方法计数 +1；**原方法体一个字节都不动**；
  * `rewrite_collect_patches()`：早期"直接替换方法体"的版本，仍保留（未在主链路使用）。
* `src/fastlaunch.rs`——**运行时链路**：
  * **`table_fn(base, index)`：唯一的环境函数表入口（本次修掉卡点的地方）**。`JavaVM` / `JNIEnv` / `jvmtiEnv` 三者运行时形态相同（都是"结构体地址，第一字段 `functions` 才是函数表"），一律 **`*(void**)base` → 按槽位索引**，调用时第一个参数仍传原始 `base`。详见 §5.4；
  * 门禁 `fast_launch_enabled_in_settings()`：读 `settings.json` 要求 `fastLaunch == true` 且 `modLoaderScheme == "zombieBuddy"`；
  * 缓存等待与校验（格式版本、`modSetHash` 非空、模组计数）；
  * **应急关闭**：`PZM_FAST_LAUNCH_DISABLE=1` 时 `Agent_OnLoad` 直接返回；
  * `Agent_OnLoad`（正式入口，`-agentpath` 加载）：门禁 → 缓存 → `GetEnv(JVMTI)` → `GetVersionNumber` 自检 → `AddCapabilities` → `SetEventCallbacks`（登记 `VMInit` + `ClassFileLoadHook`）→ **立刻** `SetEventNotificationMode(ENABLE, CLASS_FILE_LOAD_HOOK)` → `SetEventNotificationMode(ENABLE, VM_INIT)`；
  * **`vm_init_hook`**：只负责用回调给的 `JNIEnv` 做 `DefineClass` 把辅助类送进 JVM（onload 阶段拿不到 `JNIEnv`）；失败**不关闭**类加载 hook（包装体自带兜底）；
  * **`class_file_load_hook`**：类名等于 `me/zed_0xff/zombie_buddy/PatchEngine` 时，用 JVMTI 交来的原始字节 `wrap_collect_patches()` 后就地返回；带**幂等判断**（已经含 `collectPatches$pzm` 就放行，避免二次包装撞名）；第一次被调用时记一行日志（排查"hook 到底有没有生效"的关键证据）；任何失败 = 放行原字节 = 原生实现；
  * **远程注入路线（调试用，保留）**：`probe_jvm()`（`JNI_GetCreatedJavaVMs`）→ `probe_jvmti()` → `probe_attach()` → `deliver_fast_launch()` → retransform；仍要求 `PZM_FAST_LAUNCH_INJECT=1`，且 `AGENT_LOADED` 置位时整体跳过（避免重复投送）。注意这条路只能用于"直接换方法体"式改写，**不能**用于包装式装配（见 §6.2 的 retransform 限制）。
* `java/pzm/fastlaunch/FastLaunchBridge.java`——**辅助类**：
  * 与 `collectPatches` 同签名的 `collect(packageName, loader)`；命中缓存就用缓存的类名 + 传入的 `ClassLoader` 做 `Class.forName(name, false, loader)`；任何一个类名取不到 → 抛异常 → 整体回退；
  * 未命中/任何异常 → 反射调用被改名的原实现（`collectPatches$pzm`，**真正的原生回退**，不是复刻逻辑）；
  * 零第三方依赖（JSON 是手写极小解析器）；日志写 `%TEMP%\fast-launch.log`（`[bridge]` 前缀）；
  * 重新编译：`javac --release 8 -d java/classes java/pzm/fastlaunch/FastLaunchBridge.java`，产物 `include_bytes!` 进 DLL（随仓库提交，构建不依赖 javac）。

---

## 4. 运行时链路与实测日志

### 4.1 正式链路（`-agentpath` 早期加载）

```
启动器：fastLaunch=true && zombieBuddy && 覆盖层 DLL 存在
  → 把 -agentpath:<DLL> 写进 ProjectZomboid64.json 的 vmArgs（.bat 视空格情况）
  → JVM 启动最早期 LoadLibrary + 调 Agent_OnLoad(JavaVM*)
Agent_OnLoad：
  → 门禁（settings.json）+ 等/校验预扫描缓存（format=1、modSetHash 非空）
  → [PZM_FAST_LAUNCH_DISABLE=1 则到此为止]
  → GetEnv(JVMTI_VERSION_1_2) → jvmtiEnv
  → GetVersionNumber(87) 自检 → 布局自检日志
  → AddCapabilities(141)：bit26(can_generate_all_class_hook_events) + bit37(can_retransform_classes)
  → SetEventCallbacks(121)：槽 0 = VMInit、槽 4 = ClassFileLoadHook
  → SetEventNotificationMode(1, ENABLE, VM_INIT=50)
VMInit 回调（本线程已是 Java 线程，直接给 JNIEnv）：
  → DefineClass(5) 送进 pzm/fastlaunch/FastLaunchBridge（定义到 bootstrap 加载器）
  → SetEventNotificationMode(1, ENABLE, CLASS_FILE_LOAD_HOOK=54)
ZombieBuddy 自己加载 PatchEngine 时：
  → ClassFileLoadHook 回调按类名命中 → wrap_collect_patches() 就地包装
  → Allocate(45) 分配输出缓冲 → 交回改写后的字节（失败即放行原字节）
运行期：
  → Loader 调 PatchEngine.collectPatches(...) → 包装体 → FastLaunchBridge.collect(...)
     命中缓存：用缓存类名 forName(传入的 loader) —— 不跑 ClassGraph
     未命中/异常：反射调 collectPatches$pzm —— ZombieBuddy 原生实现
```

### 4.2 实测日志（本机、游戏自带 JRE = Zulu 25.0.1，`-agentpath` + 真实 `ZombieBuddy.jar`）

命令（harness 在 `%TEMP%\pzm-harness`，**不碰游戏进程**）：

```powershell
java -agentpath:F:\pzmanager\overlay-dll\target\debug\pz_overlay.dll `
     -cp "H:\SteamLibrary\steamapps\common\ProjectZomboid\ZombieBuddy.jar;$env:TEMP\pzm-harness\classes" `
     Harness call me.zed_0xff.zombie_buddy
```

`%TEMP%\fast-launch.log`：

```
Agent_OnLoad 被 JVM 调用（options=）
设置检查：fastLaunch=true modLoaderScheme=zombieBuddy
极速启动模块启动：等待预扫描缓存…
缓存可用：generatedAt=1791960000 mods=1 modSetHash=0123456789abcdef
JavaVM 布局自检：vm=0x7ffba249fd90 *(vm)=0x7ffba228e790（函数表） *(vm+8)=0x0
本进程已由 -agentpath 以 JVMTI agent 身份加载本 DLL：跳过远程注入路径（避免重复投送）
agent GetEnv(JVMTI) version=0x30010200 成功
JVMTI 布局自检：env=0x25347f439e0 *(env)=0x7ffba24a9300（函数表）
JVMTI 可用：version=0x30190000
GetEnv(version=0x00010006) 失败：status=-2 env=0x0
Agent_OnLoad 阶段没有 JNIEnv（onload 阶段线程尚未 attach），辅助类改到 VMInit 回调里定义
步骤：AddCapabilities（can_generate_all_class_hook_events + can_retransform_classes）
AddCapabilities 成功
步骤：SetEventCallbacks（VMInit + ClassFileLoadHook）
SetEventCallbacks 成功
步骤：SetEventNotificationMode(JVMTI_ENABLE, VM_INIT)
已打开 VMInit：等 JVM 就绪回调（在那里定义辅助类并打开类加载 hook）
VMInit 回调：JVM 已就绪（本线程已是 Java 线程），开始定义辅助类
辅助类已定义到 JVM：pzm/fastlaunch/FastLaunchBridge（7566 字节）
agent 已就绪：等 ZombieBuddy 加载 PatchEngine 时替换其字节码（失败即放行原字节）
ClassFileLoadHook：已提交改写后的 PatchEngine（短路生效）
[bridge] 缓存已载入：包数=1
```

harness stdout（同一进程）：

```
[harness] java.version=25.0.1
[harness] loaded me.zed_0xff.zombie_buddy.PatchEngine loader=jdk.internal.loader.ClassLoaders$AppClassLoader@…
[harness] method: public static java.util.List …PatchEngine.collectPatches$pzm(java.lang.String,java.lang.ClassLoader)
[harness] method: public static java.util.List …PatchEngine.collectPatches(java.lang.String,java.lang.ClassLoader)
[harness] RESULT wrap=true
[harness] collectPatches(me.zed_0xff.zombie_buddy) -> [class me.zed_0xff.zombie_buddy.PatchEngine]
```

* 三条结论：① 包装后的类**被 JVM 接受并成功加载**（否则 `getDeclaredMethods` 会抛 `ClassFormatError`/`VerifyError`）；② **缓存命中时不再出现 `[ZB] Scanned …`**（= ZombieBuddy 的 ClassGraph 扫描真的被短路）；③ 退出码 0、无崩溃。
* 对照：把缓存里的 `basePackage` 换成一个不存在的包，同一命令会打印 ZombieBuddy 自己的
  `[ZB] Scanned 0 classes in package pzm.harness.nonexistent`，并正常返回 `[]`
  —— 这条证明"缓存未命中 → 反射调原生实现"这条回退是**真的原生实现**在跑。

---

## 5. 已实证的常量（全部取自本机头文件，不要凭记忆）

### 5.1 JNI（`jni.h` 的 `JNINativeInterface_`，**含 reserved 槽位的绝对下标**）

`DefineClass`=5、`FindClass`=6、`ExceptionClear`=17、`DeleteLocalRef`=23、`GetObjectClass`=31、
`GetStaticMethodID`=113、`CallStaticObjectMethodA`=116、`CallStaticVoidMethod`=141、
`NewStringUTF`=167、`ExceptionCheck`=228。

> 可变参数的 `CallStaticObjectMethod`(114) 在 Rust 里**无法声明函数指针类型**，所以只能用 A 版 + `jvalue` 数组。

### 5.2 `JNIInvokeInterface_`（`JavaVM` 的函数表）

0–2 reserved、3 `DestroyJavaVM`、4 `AttachCurrentThread`、5 `DetachCurrentThread`、
**6 `GetEnv`**、7 `AttachCurrentThreadAsDaemon`。

### 5.3 JVMTI（`jvmti.h` 的 `jvmtiInterface_1_`，**含 reserved**）

`SetEventNotificationMode`=1、`GetAllThreads`=3、`Allocate`=45、`Deallocate`=46、
`GetClassSignature`=47、`GetClassStatus`=48、`GetClassMethods`=51、`IsInterface`=54、
`GetMethodName`=63、`GetLoadedClasses`=77、`GetClassLoaderClasses`=78、`GetVersionNumber`=87、
`SetEventCallbacks`=121、`GetPotentialCapabilities`=139、`AddCapabilities`=141、
`RetransformClasses`=151。（`struct jvmtiInterface_1_` 共 **307** 个槽位，逐条数过。）

### 5.4 环境指针的形态（**本次卡点的根因，务必先读**）

JVM 交到手上的 `JavaVM` / `JNIEnv` / `jvmtiEnv` **三者形态完全一样**，都不是"指针即函数表"：

```c
struct JavaVM_   { const struct JNIInvokeInterface_ *functions; … };  // typedef JavaVM_ JavaVM;    (C++)
struct JNIEnv_   { const struct JNINativeInterface_ *functions; … };  // typedef JNIEnv_ JNIEnv;    (C++)
struct _jvmtiEnv { const struct jvmtiInterface_1_   *functions; … };  // typedef _jvmtiEnv jvmtiEnv; (C++)
```

C 下这三个 typedef 是"指向函数表的指针"，**照 C 的签名去读就会少解一层**：

* 取函数指针：`table = *(void**)base;` 然后 `table[index]`（`table_fn()`）；
* 调用时第一个参数仍是**原始 base**（HotSpot 的 C++ 实现也是 `functions->X(this, …)`）。

实测证据：

| 现象（旧代码：把 base 直接当函数表） | 原因 |
| --- | --- |
| `GetEnv`（槽位 6）返回 `status=1376256 (0x150000)`、`env=0x0`，四个 JVMTI 版本 + JNI 版本全都一样 | 读到的是 `JavaVM_` 结构体后面内存里的无关全局量（实测 `vm+0x30`）并**调用它** |
| `AttachCurrentThread`（槽位 4）崩在 `jvm.dll` 内（`0xC0000005`） | 同上，槽位 4 也落在无关内存上 |
| `jvmtiEnv` 的 `GetVersionNumber`（槽位 87）在 `Agent_OnLoad` 里**卡死**（JVM 再也起不来） | 同上，`_jvmtiEnv` 结构体 +696 字节处不是函数指针 |

布局自检日志（代码里保留，别删）：`*(JavaVM)` 与 `*(jvmtiEnv)` 都应当是 jvm.dll 里的函数表地址，
`*(JavaVM+8)` 为 0（实测 `vm=0x7ffb…fd90`、`*(vm)=0x7ffb…e790`、`*(vm+8)=0x0`）。

### 5.5 其它

* `JVMTI_VERSION_1_2 = 0x30010200`、`1_1 = 0x30010100`、`1_0 = 0x30010000`；
  游戏自带 JRE 是 **Zulu 25.0.1（JVMTI 0x30190000）**，`1_2` 一样被接受；
* `JVMTI_EVENT_VM_INIT = 50`、`JVMTI_EVENT_CLASS_FILE_LOAD_HOOK = 54`、`JVMTI_ENABLE = 1`、`JVMTI_ERROR_NONE = 0`；
* `jvmtiEventCallbacks` 共 **39** 槽：**`VMInit` 在槽位 0**、`ClassFileLoadHook` 在槽位 4；
* `jvmtiCapabilities` 共 45 位（2 个 `u32`）：`can_redefine_classes`=9、`can_redefine_any_class`=21、
  `can_generate_all_class_hook_events`=26、**`can_retransform_classes`=37**（即 `bits[1]` 的 bit 5）、
  `can_retransform_any_class`=38、`can_generate_early_class_hook_events`=42；
* `ClassFileLoadHook` 回调签名：`(jvmtiEnv*, JNIEnv*, jclass class_being_redefined, jobject loader, const char* name, jobject protection_domain, jint class_data_len, const unsigned char* class_data, jint* new_class_data_len, unsigned char** new_class_data)`；
* `Agent_OnLoad` 阶段 `GetEnv(vm, JNI_VERSION_1_6)` 返回 **`-2`（`JNI_EDETACHED`）**：那时线程还不是 Java 线程。

---

## 6. 本次会话定位的根因（**推翻**上一版结论）

上一版的结论（当时写在 §6.0 一节里，已随本次改写被删除）是"JDK 9+ 下 `GetEnv(JVMTI_VERSION)` 只对 JVMTI agent 开放，
所以必须走 `-agentpath`"。**这个结论是错的**：

* 真正的根因是 `JavaVM`/`jvmtiEnv` 的函数表**少解了一层指针**（§5.4），于是所有槽位调用都落在无关内存上；
* 换成 `-agentpath` 之后"仍然失败"，只是因为**同一处 bug 在 agent 路径里也在**（`jvmti_env_direct` 也把 `vm` 当函数表）；
* 决定性证据其实上一轮就打印出来了，只是没被读对：
  `vm+0x00 -> 0x00007ffba22eb240`、`vm+0x08..0x28 -> 0`、`vm+0x30 -> 0x00007ffba1d29e00`
  —— 第一槽是"jvm.dll 数据段里的函数表地址"、后面是 0，这正是 `struct JavaVM_ { functions; … }` 的样子；
  旧代码却把 `vm+0x30` 当成 `GetEnv` 调用了。

**方法论教训（写给下一个会话）**：

1. 下标核对只解决一半问题；**指针层级必须用运行时内存布局自检确认**，别靠"照抄 JNI 的写法/反过来"猜；
2. 出现"同一槽位、传不同版本号全部返回同一个垃圾值"时，优先怀疑**读错了函数表**，而不是 VM 的限制；
3. 别把"未知的返回值"解释成"平台限制"：`0x150000` 既不是 JNI 错误码也不是 JVMTI 错误码，
   这本身就是"调用了错误函数"的信号。

---

## 6.1 第二轮实测：游戏里"日志全绿但没加速"——hook 一次都没触发

现象（2026-10-14 23:44 / 23:54 两次真实启动，`fastLaunch=true`、缓存 26 个模组）：

* `%TEMP%\fast-launch.log` 显示 agent 全链路成功：`GetEnv(JVMTI)`→`AddCapabilities`→
  `SetEventCallbacks`→`SetEventNotificationMode(VM_INIT)`→`VMInit 里 DefineClass 辅助类`→
  "agent 已就绪"；
* **但整局都没有 `ClassFileLoadHook：已提交改写后的 PatchEngine`**，也没有 `[bridge]` 行；
* 游戏照旧打 `[ZB] Scanned N classes in package …`（原生 ClassGraph 全量扫描），所以启动耗时没变。

定位（用带毫秒时间戳的 `%USERPROFILE%\Zomboid\Logs\logs_*/…_DebugLog.txt`）：

```
23:44:06  Agent_OnLoad（pid=32108）
23:44:07  VMInit → 辅助类定义成功 → （当时的代码在这里才打开类加载 hook）
23:44:08.225  游戏自己的日志开始
23:44:08.882  [ZB] patching zombie.GameWindow.init …   ← ZB 已经在跑补丁
23:44:36.636  [ZB] Scanned 1079 classes in package viewpoint
```

ZB 的补丁工作在游戏日志开始之前就开始了，而**所有 agent 的 `Agent_OnLoad` 都在 `VMInit` 之前**：
`vmArgs` 里 `-agentlib:zbNative=policy=allow-all` 排在前面，ZB 在**它自己的 onload 阶段**就把
`PatchEngine` 加载好了；我们的 hook 等到 `VMInit` 才打开，那时类早就定义完了 →
**hook 永远等不到那次定义**（这解释了"全链路成功但毫无效果"）。

## 6.2 修法（三件必须同时成立的事）

1. **`-agentpath:` 必须排在 `vmArgs[0]`**：JVMTI agent 按命令行顺序加载，只有排在 ZB 的
   `-agentlib:zbNative` 之前，我们的 `Agent_OnLoad` 才会先执行。
   `normalize_fast_launch_agent_tokens()` 现在会把这条参数**插到最前**（有单测断言位置）；
   ZB 自己的 `policy=allow-all` token 仍然保留、仍然是唯一的 `zbNative` token。
2. **类加载 hook 在 `Agent_OnLoad` 里就打开**（不能等 `VMInit`，它晚于所有 agent 的 onload）。
3. **包装体自带 `catch (Throwable)` 兜底**：hook 打开的时机比"辅助类能进 JVM"的时机早
   （onload 拿不到 `JNIEnv`），中间可能已经有 `collectPatches` 调用。兜底让这些调用走被改名的
   原实现，而不是抛 `NoClassDefFoundError` 把加载器弄坏。包装体现在是：

```text
  0: aload_0 / aload_1 / invokestatic FastLaunchBridge.collect / areturn
  6: pop / aload_0 / aload_1 / invokestatic PatchEngine.collectPatches$pzm / areturn
  异常表：[0,6) -> 6，catch_type = java/lang/Throwable（必须配 StackMapTable，否则 VerifyError）
```

**不能用 `RetransformClasses` 事后补救**：JVMTI 规范规定 retransform 不得
"add, remove or rename fields or methods, change the signatures of methods, change modifiers,
or change inheritance"（见 [JVMTI 规范修订记录](https://cr.openjdk.org/~sspitsyn/webrevs/2018/8191118-svc-nestmates.4/open.patch)），
而"包装式装配"恰恰是**新增方法 + 改名**，所以只能在**类加载那一刻**改写。

harness 实测（同一份 release DLL，游戏自带 JRE）：
* 正常路径：`ClassFileLoadHook 首次被调用（首个类=jdk/internal/vm/ContinuationSupport）` →
  `已提交改写后的 PatchEngine（31276 -> 31532 字节）` → `collectPatches` 命中缓存直接返回，**没有** `[ZB] Scanned`；
* `PZM_FAST_LAUNCH_SKIP_BRIDGE=1`（故意不定义辅助类，专测兜底）：包装仍然成功，
  `collectPatches` 打出 `[ZB] Scanned 195 classes …` 并抛出与**完全不注入时逐字相同**的异常
  —— 说明兜底确实回到 ZombieBuddy 的原实现，而不是把加载器弄坏。

**游戏内验证了 §6.2 的修法有效**（2026-10-15 13:04 会话，`-agentpath` 已在最前）：
`%TEMP%\fast-launch.log` 里出现了 `类加载 hook 已打开` → `ClassFileLoadHook 首次被调用` →
`VMInit：辅助类就位；PatchEngine 还没被定义` → `ClassFileLoadHook：已提交改写后的 PatchEngine（31276 -> 31532 字节，短路生效）`
—— **包装第一次真的落到了游戏进程里的 `PatchEngine` 上**（这正是上一轮缺的那一步）。

## 6.3 第三轮实测：包装成功了，但缓存路径对每个模组都落空（类名格式 bug）

同一个 13:04 会话的日志紧接着是：

```
[bridge] 缓存已载入：包数=25
[bridge] 缓存路径失败，回退原生扫描: java.lang.ClassNotFoundException: viewpoint/Patch_AimAxis
[bridge] 缓存路径失败，回退原生扫描: java.lang.ClassNotFoundException: pvads/Patches$AimButton
…（25 个模组，每条都是同一个形状）
```

也就是说：**每个模组的 `collectPatches` 都走回了原生 ClassGraph 全量扫描**，所以"还是没加速"。
DebugLog 里也能直接看到 `[ZB] Scanned 1079 classes in package viewpoint` 照旧出现。

根因：缓存里的 `patchClasses` 存的是**内部名**（`viewpoint/Patch_AimAxis`，斜杠分隔 ——
`class_annotations()` 给的就是这个形式），而桥接类把它直接喂给了
`Class.forName(name, false, loader)`，后者只认**二进制名**（点号）→ 一律 `ClassNotFoundException`。

为什么上一轮的 harness 没暴露它：我当时手写的测试缓存用的是**点号**类名
（`me.zed_0xff.zombie_buddy.PatchEngine`），正好绕过了这个坑。**教训：测试数据要照真实产物的形状来。**

修法（两边都改，互为兜底）：
1. 桥接类 `FastLaunchBridge.collect` 里加 `binaryName()`：`/` → `.`（老缓存也能救回来）；
2. 启动器 `build_fast_launch_cache` 里加 `binary_class_name()`：写缓存时就落成点号；
3. 顺手把桥接类的**沉默回退**改成有日志：`lookup` 未命中现在会打
   `[bridge] 缓存无此包，走原生扫描：<包名>`，命中则打
   `[bridge] 命中缓存：<包名> → N 个类（跳过 ClassGraph 扫描）`。
   （沉默回退让我们误判过一轮，这类"优化没生效"的路径必须能从日志里看出来。）
4. 单测 fixture 从点号改成**内部名**（和真实扫描一致），断言缓存里落成点号 —— 防止再犯。

**当前状态（交接时的准确口径）**：
* 代码改动与单测都已完成（`overlay-dll` 14 passed；`src-tauri` 109 passed / 2 ignored；前端 32 通过 / 4 既有基线失败）；
* DLL 三个变体（debug / release / release+x86_64-pc-windows-msvc）都已重新构建（`FastLaunchBridge.class` 7935 字节已 `include_bytes!` 进 DLL）；
* **但"真实形状缓存 → 命中"这条端到端复验没有跑完**（最后一次 harness 命令被用户中断）。
  下一次必须补跑：harness（缓存用真实形状）或直接进游戏看
  `[bridge] 命中缓存：…` 与 `[ZB] Scanned` 是否消失。

## 6.4 "20 秒左右跳转加载完成、然后界面卡住"的排查结论（未定论）

对比两个会话的 DebugLog（都带毫秒时间戳）：

| 会话 | 结果 | 关键时间线 |
| --- | --- | --- |
| 00:17（正常，`-agentpath` 还在末尾，**没有**包装成功） | 日志 9431 行、跑到 shader 编译 | 游戏日志 00:17:17 → 第一次 ZB 扫描 **00:17:37**（+20 s） |
| 13:04（卡住，`-agentpath` 已在最前，**包装成功**） | 日志 1742 行，在 ZB 打补丁途中**中断** | 游戏日志 13:04:23 → 第一次 ZB 扫描 **13:05:33**（+70 s） |

那 50 秒的差距**不是极速启动造成的**，日志里能直接看到两个外部原因：
`[pzopt] update prefetch … 600 of 1176 files differ in 15960 ms`（PZ Optimization 在启动过程中
自我更新了 600 个文件，16 s）+ `[ZB] Authors list fetch failed: HTTP connect timed out`（ZB 联网超时，11 s）。

卡住的证据与未知点：
* 13:04 会话的日志停在 `[ZB] patching viewpoint.render.WorldRenderer.finish with 2 advice(s)`，
  只打了 400 条 patching（00:17 会话是 900 条并继续到 shader 编译）→ 确实是在 ZB 打补丁阶段停住的；
* **没有任何 `hs_err_pid*.log`、没有 JVM 崩溃记录** → 进程是被结束的（用户看到界面卡住后关掉）；
* 两次会话的字体错误（`AngelCodeFont failed to load page 0 texture mainfont_0.png` 等）数量相近，
  是既有现象，与本次改动无关；
* **尚未确定卡住是否与注入有关**。唯一新增的变量是"我们的类加载 hook 从 VM 初始化阶段就一直开着"
  （因为 `-agentpath` 排到了最前），以及"启动期间 pzopt 换掉了 600 个文件"。

下一次的判定办法（**按顺序做，一次只改一个变量**）：
1. 什么都不动，再启动一次游戏（让 pzopt 的自我更新落定）：若这次正常进入主菜单，说明卡住来自
   "启动中途换文件 + 刚好赶上"，与我们无关；
2. 若仍然卡住：设置里**关掉「极速启动」**再启动一次（启动器会在这次启动前把 `-agentpath:` 摘掉，
   等于完全不注入）。不卡了 ⇒ 与注入有关，继续查；照旧卡 ⇒ 是模组/pzopt 侧的问题；
3. 想同时验证"功能生效"，见 §10.1（需要先点一次「刷新 JAR 列表」重建缓存，见下面的提醒）。

⚠️ **交接提醒（我造成的）**：本轮为了做回归测试，我把 `%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`
**覆盖又删掉了**（用户 13:04 那次扫描生成的缓存也被覆盖了）。所以下一次测试前必须先在
「模组管理 › JAR」重新点一次「刷新 JAR 列表」。以后做这类测试务必先备份这个文件。

---

## 6.5 2026-10-15 深夜：改成"强制模式"（默认不回退，用户明确要求）

用户要求：**不回退，强制使用启动器数据**。桥接类 `FastLaunchBridge.collect` 因此改成：

| 情形 | 改前 | 改后（默认） |
| --- | --- | --- |
| 缓存命中 | 用缓存类名 forName（走 `binaryName()`） | 不变 |
| 缓存命中但某个类名取不到 | 抛异常 → **整包**回退原生扫描 | 跳过那一个、记日志，**不回退** |
| 缓存里没有这个包 | 反射调 `collectPatches$pzm`（原生 ClassGraph） | 记一行、返回**空列表** |
| 缓存缺失/格式不符/无指纹 | 全部回退原生扫描 | 每个包返回**空列表** |

逃生开关：`PZM_FAST_LAUNCH_ALLOW_FALLBACK=1` 恢复旧行为（环境变量必须能进游戏进程 ——
设成系统环境变量后重启 Steam，或在"启动游戏"的那个 shell 里设）。

**代价（显式接受）**：缓存不全时，那些包的补丁**直接缺失**，而不是悄悄回退 —— 这正是目的，
回退会让"没加速"从日志上看不出来。所以改完模组集合必须点一次「刷新 JAR 列表」。

**harness 实测（本机 `%TEMP%\pzm-harness`，不碰游戏进程）**：

* 命中缓存：`collectPatches(me.zed_0xff.zombie_buddy) -> [class me.zed_0xff.zombie_buddy.PatchEngine]`
  —— 缓存里存的是内部名 `me/zed_0xff/.../PatchEngine`，证明 `binaryName()` 转换真的生效（此前从未被真实命中验证过）；
* 未命中 + 强制模式：`collectPatches(me.zed_0xff.zombie_buddy.patches) -> []`，
  日志 `强制模式（不回退）：缓存里没有这个包或缓存不可用，返回空列表`；
* 未命中 + `PZM_FAST_LAUNCH_ALLOW_FALLBACK=1`：同一包返回 **11 个** ZB 自带 patch 类，证明逃生开关有效。

> ⚠️ 这只改了"查不到怎么办"，**没有**解决"缓存本身是假夹具"的问题（见 §6.3 交接提醒）。
> 真加速仍然要求先点「刷新 JAR 列表」生成真实缓存；在强制模式下，假缓存会让**所有**包返回空列表。

---

## 6.6 2026-10-15 深夜（二）：把 ZombieBuddy 自己并进缓存 + 支持子包

用户实测反馈：强制模式下进游戏弹 **"ZombieBuddy 未安装"**。根因两条，都是"缓存里没有 ZB 自己"：

1. **加载器本体被排除在 JAR 模组列表外**：`jar_scan_candidates` 按 `ZOMBIE_BUDDY_WORKSHOP_ID`
   跳过 workshop `3619862853`（这是**有意**的——它是方案本身，不是可审批的模组），
   但这个函数同时也决定了预扫描缓存的内容，于是 ZB 自己的补丁类从来没进过缓存。
2. **包名对不上**：ZB 的 `mod.info` 声明 `javaPkgName=me.zed_0xff.zombie_buddy`，
   而它调的是 `collectPatches("me.zed_0xff.zombie_buddy.patches")`（补丁类在**子包**里）；
   旧扫描又只收"正好等于声明包"的类，把子包整批漏掉。

改法（启动器 + 桥接类两侧一起）：

* `scan_java_mod_jar`：收声明包**及其子包**的类（ClassGraph 的 `acceptPackages` 本来就是递归的）；
* 新增 `zombie_buddy_loader_cache_mod_at` / `fast_launch_cache_mods_at`：把加载器本体并进缓存；
  **「刷新 JAR 列表」写缓存与设置页校验指纹走同一个函数**，否则指纹永远对不上（"一直显示建议重建"）；
  界面上的 JAR 模组列表仍然排除它（`excludes_zombie_buddy_loader_from_jar_mod_candidates` 不变）；
* 桥接类 `loadCache`：按**类自己的包**建索引（不再用 `basePackage`）；
  `lookup(pkg)`：返回 `pkg` **及其子包**里的类（对齐 ClassGraph 语义）。

**harness 实测**（合成一份"真实形状"的缓存：`basePackage=me.zed_0xff.zombie_buddy`，
`patchClasses=...patches.Patch_Core$...`）：

| 查询 | 结果 |
| --- | --- |
| `me.zed_0xff.zombie_buddy` | 命中 2/2（父包递归到子包） |
| `me.zed_0xff.zombie_buddy.patches` | 命中 2/2（**改前永远 miss**） |
| `viewpoint` | 未命中 → 强制模式返回空列表 |

单测：`src-tauri` **110 passed**（新增 `includes_zombie_buddy_loader_and_subpackages_in_fast_launch_cache`）；
`overlay-dll` **14 passed**。

> ⚠️ 仍然要**先点「刷新 JAR 列表」**：磁盘上现有的缓存（假夹具或旧缓存）里没有 ZB，
> 不刷新的话强制模式依旧会把 ZB 的补丁丢掉。

---

## 6.7 2026-10-15 深夜（三）：`loader == null` —— 43/43 全部 ClassNotFoundException

§6.6 修好后缓存里已经有 ZB 的 43 个补丁类，但 23:41 那次启动的日志是：

```
[bridge] 命中缓存：me.zed_0xff.zombie_buddy.patches → 0/43 个类（跳过 ClassGraph 扫描）
[bridge] 类名取不到，跳过（不回退整包）：me.zed_0xff.zombie_buddy.patches.Patch_Core$... → java.lang.ClassNotFoundException: ...
```

43 条全是"类名取不到"，而且**整局只有这一次 `collectPatches` 调用** —— ZB 自己的补丁一个都没装上，
它后面根本没走到扫描那 25 个模组，游戏里就弹"ZombieBuddy 未安装"。

**根因**：ZombieBuddy 传进来的 `loader` 就是 `null`。反编译 `ZombieBuddy.jar` 的 `Loader`：

```
loadJar(Path, String, String, Phase):
  55: addJarToClasspath(...)      // Instrumentation.appendToSystemClassLoaderSearch(jar)
  66: aload_1                     // package
  67: aconst_null                 // ← loader = null
  69: invokestatic ApplyPatchesFromPackage:(String, ClassLoader, Phase)
→ ApplyPatchesFromPackage → PatchEngine.applyPatches(pkg, null) → collectPatches(pkg, null)
   （collectPatches 里 loader==null 时走 overrideClasspath(getCurrentJarPath + g_known_jars)）
```

`Class.forName(name, false, null)` 用的是 **bootstrap 加载器** → 一个都找不到。
ZB 原生能行，是因为它把每个 jar 都 `appendToSystemClassLoaderSearch` 到**系统加载器**上了。

**修法**：桥接类新增 `resolveClass()`，按 `调用方 loader → 系统加载器 → 线程上下文加载器`
依次尝试；`loader == null` 时直接用系统加载器（与 ZB 原生一致），成功时记一行
`调用方给的加载器是 null，改用系统/上下文加载器取类`。

harness 实测（真实缓存 + 真实 jar，**显式传 null**）：

| 查询（`loader = null`） | 结果 |
| --- | --- |
| `me.zed_0xff.zombie_buddy.patches` | **43/43** |
| `viewpoint`（Viewpoint.jar 在 classpath 上） | **92/92** |
| `com.viewpointmovement`（jar 不在 classpath） | 0/35（harness 限制；游戏里 ZB 会把它的 jar 挂上系统加载器） |

---

## 6.8 2026-10-15 深夜（四）：游戏崩溃 —— `duplicate class definition for zombie.vehicles.UI3DScene`

23:50 那次启动（加载器修好、补丁真的装上了）游戏在 23:52:11 崩了。没有 `hs_err`，
不是 JVM 硬崩，是游戏自己抛的：

```
[ZB] Transformed: zombie.vehicles.UI3DScene (new load)          ← 3ms 前
ERROR GameWindow.uncaughtException > Unhandled java.lang.LinkageError
java.lang.LinkageError: loader 'app' attempted duplicate class definition for
    zombie.vehicles.UI3DScene. (… unnamed module of loader 'app')
  at BuiltinClassLoader.findClassOnClassPathOrNull
  at zombie.Lua.LuaManager$Exposer.exposeAll(LuaManager.java:2527)
  at zombie.Lua.LuaManager.init(LuaManager.java:1130)
```

**关键对照**：所有**原生扫描**的运行里同一行都是 `(retransformed)`，且都能进主菜单
（00:17、22:41 各验证一次）；只有"补丁清单瞬间返回"的这次是 `(new load)`，然后崩。

**机制（从 ZB 字节码读出来的）**：`PatchEngine.applyPatches` 装的是
`AgentBuilder.Default().disableClassFormatChanges().with(RedefinitionStrategy.RETRANSFORMATION).installOn(instrumentation)`，
并且会先 `Class.forName(目标类)` 再 `retransformClasses` / `warmUp`。原生扫描要跑几十秒，
这期间游戏自己的线程已经把 `UI3DScene` 加载好了 → ZB 走 retransform（原地替换，安全）；
极速启动把补丁清单瞬间返回，ZB 就撞上"还没加载"的 `UI3DScene` → 走 ByteBuddy 的 new load
路径，紧接着游戏自己的 `Exposer.exposeAll` 再 loadClass 同一个类 → duplicate。

**实验开关（默认关）**：`PZM_FAST_LAUNCH_PRELOAD_TARGETS=1`，或在 `%TEMP%\` 放一个
`pzm-fast-launch-preload.flag`。打开后桥接类在 `collect()`（此时 ZB 的 ByteBuddy agent
还没 `installOn`）里把每个 `@Patch(className=…)` 的目标类用系统加载器
`Class.forName(target, false, …)` 先加载进来，逼 ZB 走 retransform 路径。
成功时 `%TEMP%\fast-launch.log` 会出现 `已提前加载 N 个 @Patch 目标类`。

> ⚠️ 这是**实验**，没验证过能不能消掉这次崩溃。如果无效，退路是把设置里的
> 「极速启动」关掉（或 `PZM_FAST_LAUNCH_DISABLE=1`）—— 原生扫描路径历史上一律正常。

---

## 7. 安全状态与红线

* **开关（玩家可见）**：设置 › 启动 › 极速启动。只有它打开 **且** 方案是 `zombieBuddy` **且** 覆盖层 DLL
  存在时，启动器才会写 `-agentpath:`；任一不成立都会**摘掉**已有参数（`-agentpath` 指向的文件不存在
  会让 JVM 直接启动失败，所以"关开关必须真的清干净"）。关开关时**当场**清理，不等下一次启动；
* **应急关闭**：环境变量 `PZM_FAST_LAUNCH_DISABLE=1` 让 agent 立刻什么都不做（不需要改任何文件）；
* **门禁**：注入器自己也读 `settings.json` 再判一次（`fastLaunch == true && modLoaderScheme == zombieBuddy`）；
  前端的禁用只是界面约束；
* **回退（2026-10-15 深夜改：默认不回退）**：辅助类默认是**强制模式** —— 只用启动器预扫描数据，
  缓存里没有这个包 / 缓存缺失 / 格式不符 / 指纹为空，一律返回**空列表**，不调用原生实现；
  缓存命中但某个类名取不到只跳过那一个。设环境变量 `PZM_FAST_LAUNCH_ALLOW_FALLBACK=1`
  才恢复旧行为（反射调 `collectPatches$pzm` = **ZombieBuddy 自己的实现**，见 §6.5）。
  唯一的例外仍在包装体（`classfile.rs`）：辅助类**根本没能进 JVM** 时，包装体的
  `catch (Throwable)` 会走原实现，避免 `NoClassDefFoundError` 把加载器弄坏；
* **缓存的持久化策略**：缓存一旦生成就留着，**只有「刷新 JAR 列表」会重建**；改开关、改审批、
  改启用/禁用都不再删它（旧的"一改就删"已移除）。指纹不一致时状态行会提示重建，但**不会**自动放弃缓存；
* **已知的残余风险（唯一一个）**：改了模组却没刷新 JAR 列表时，缓存里的 `patchClasses` 可能过期
  （例如某个模组更新后新增了 `@Patch` 类，而旧类名仍能加载成功）→ 那部分补丁会缺失。
  强制模式下**不再有"回退原生扫描"来兜底**：新启用的模组在刷新前就是没有补丁。
  发现某个模组补丁失效时，点一次「刷新 JAR 列表」即可，或关掉开关；
* **远程注入路线**（attach + retransform）仍是**调试路径**：`PZM_FAST_LAUNCH_INJECT=1` 才执行，
  且 agent 一旦接管（`AGENT_LOADED`）就整体让路；
* **用户当前设置**（2026-10-15 凌晨）：`modLoaderScheme = "zombieBuddy"`、`fastLaunch = true`；
  游戏启动文件里的 `-agentpath:` 是上一轮旧构建写的（在 `vmArgs` **末尾**），
  下一次用启动器「启动游戏」会把它挪到**最前面**（这一版的关键修复之一）。

---

## 8. 测试与验证现状

| 位置 | 命令 | 结果 |
| --- | --- | --- |
| `overlay-dll` | `cargo test` | **14 passed / 0 failed**（含真实 `PatchEngine.class` 包装式改写逐字节断言、新包装体的异常表与 StackMapTable 逐字节断言、javac 产物解析、缓存/门禁解析） |
| `src-tauri` | `cargo test --lib` | **110 passed / 0 failed / 2 ignored**（`-agentpath` 用例：写入/幂等/去重/**必须在最前**/摘除、.bat 空格路径、缓存持久化策略、**缓存类名落成二进制名**、**加载器本体与子包进缓存**） |
| 前端 | `npm run build` / `npm test` | 构建 ✓；36 测试 / 32 通过 / 4 失败（4 个是既有基线失败） |
| 运行时（正常路径） | 一次性 JVM harness | ✅ 类加载 hook 在 `Agent_OnLoad` 打开 → `VMInit` 定义辅助类 → `PatchEngine` **在加载时**被改写（31276→31532 字节）→ 缓存命中直接返回、无 `[ZB] Scanned` |
| 运行时（兜底路径） | `PZM_FAST_LAUNCH_SKIP_BRIDGE=1` + harness | ✅ 辅助类缺席时包装体 `catch` 生效：行为与**完全不注入**逐字相同，不会抛 `NoClassDefFoundError` |
| 运行时（游戏内包装） | 13:04 真实启动 | ✅ 包装第一次落到游戏进程的 `PatchEngine` 上（§6.2 的修法有效） |
| 运行时（游戏内缓存命中） | 13:04 真实启动 | ❌ **失败**：25 个模组全部 `ClassNotFoundException` 回退原生扫描（§6.3，已修但**未复验**） |
| 游戏内（修完类名格式） | 未做 | 见 §10.1 |

验证注意（仍然成立）：**`cargo test` 不会重新链接 cdylib**，验证前必须 `cargo build`，否则加载的是旧 DLL。

---

## 9. 文件清单（本功能相关）

```
src-tauri/src/lib.rs                     缓存构造/读/写/一致性/失效、save_fast_launch、fast_launch_cache_status、
                                         -agentpath 归一化 + launch_project_zomboid 接线 + resolve_fast_launch_agent_path
src/App.tsx                              设置 › 启动 分类、极速启动开关、缓存状态行、启动时把 DLL 路径透传给后端
overlay-dll/Cargo.toml                   新增 jni = "0.21"（不开 invocation feature）
overlay-dll/src/fastlaunch.rs            table_fn（三种环境的函数表）、agent 链路、VMInit 回调、类加载回调、远程调试链路
overlay-dll/src/classfile.rs             字节码工具链 + 包装式装配（10 个单测）
overlay-dll/java/pzm/fastlaunch/FastLaunchBridge.java   辅助类（`binaryName()` 内部名→二进制名、命中/未命中都记日志、**默认强制模式不回退** + `PZM_FAST_LAUNCH_ALLOW_FALLBACK` 逃生开关）
overlay-dll/java/classes/pzm/fastlaunch/FastLaunchBridge.class   编译产物（7935 字节，`include_bytes!` 用；改了 .java 必须重跑 javac）
docs/architecture.md §6.3                规格、短路点、下标表、环境指针层级结论、缓存生命周期
docs/fast-launch-status.md               本文档
```

---

## 11. 本轮（2026-10-15）改动的文件清单

```
src-tauri/src/lib.rs                     binary_class_name()（缓存落点号类名）、should_write_fast_launch_cache()
                                         （缓存只在显式刷新时重建）、start_zombie_buddy_jar_scan(refresh_cache)、
                                         -agentpath 归一化改成"插到最前"、save_fast_launch(false) 当场摘参数
src/App.tsx                              loadJarMods(refreshCache)、「刷新 JAR 列表」按钮、缓存状态行/说明文案
overlay-dll/java/pzm/fastlaunch/FastLaunchBridge.java   binaryName() + 命中/未命中日志
overlay-dll/java/classes/.../FastLaunchBridge.class     重新编译（javac --release 8，include_bytes! 进 DLL）
overlay-dll/src/fastlaunch.rs            类加载 hook 提前到 Agent_OnLoad、命中/首次调用/幂等日志、PZM_FAST_LAUNCH_SKIP_BRIDGE
overlay-dll/src/classfile.rs             包装体加 catch(Throwable) 兜底 + 异常表 + StackMapTable、declares_method()
docs/architecture.md / docs/fast-launch-status.md        同步
```

---

## 10. 一句话交接

**链路本身已经全部打通，剩下的都是"最后一公里"的细节**：① JVMTI 环境指针层级（§5.4）—— 决定
`GetEnv` 能不能用；② hook 必须在 `Agent_OnLoad` 就打开、`-agentpath` 必须排在最前（§6.2）—— 决定
`PatchEngine` 会不会被改写（13:04 会话已证明**改写真的落到了游戏进程里**）；③ 缓存里的类名必须是
**二进制名（点号）**（§6.3）—— 决定缓存路径命中还是每个模组都回退原生扫描。
这三处现在都改好了（改动 + 单测 + DLL 重建都完成），**但"游戏里真的加速"还没有被任何一次实测证明**：
13:04 那次包装成功却因为 ③ 全部回退；③ 修完之后的复验被中断。

**下一轮接手请按这个顺序做（每步都能单独判定）**：

1. 先在「模组管理 › JAR」点一次「**刷新 JAR 列表**」——**上一轮测试把缓存文件删掉了，必须先重建**
   （设置页状态行会显示时间/模组数/指纹）；
2. 再启动一次游戏，看 `%TEMP%\fast-launch.log` 是否出现
   `[bridge] 命中缓存：<包名> → N 个类（跳过 ClassGraph 扫描）`；若仍是
   `[bridge] 缓存路径失败…` 或 `[bridge] 缓存无此包…`，把那几行原文贴出来即可继续定位；
3. 对照 `Logs\<最新>\…_DebugLog.txt`：模组包的 `[ZB] Scanned N classes in package X` 应当消失；
4. 若界面卡住（§6.4），按那里的 A/B 步骤判定是否与注入有关，再决定要不要继续修。

**长期待办**（与"能不能生效"无关）：把 `pz_overlay.dll` 纳入发布产物、并在设置里持久化它的路径
（目前路径来自前端开发期常量 `DEV_OVERLAY_DLL_PATH`，发布包不含这个 DLL）；以及让非开发用户
不必关心缓存重建时机。

### 10.1 复现步骤（下一轮实测照这个做）

1. 设置 › 启动 › 打开「极速启动」（当前是**开**；关着的时候启动器会把 `-agentpath:` 摘掉）；
2. 在「模组管理 › JAR」点一次「**刷新 JAR 列表**」：这是**唯一**会重建预扫描缓存的入口
   （自动扫描只在缓存不存在时补一份，不会替换已有缓存；设置页的缓存状态行会显示时间/模组数/指纹）；
3. 用启动器里的「**启动游戏**」按钮启动（只有它会写 `-agentpath:`，并且会把它放到 `vmArgs` 最前面；
   直接从 Steam 启动不会改写启动文件）；
4. 观察两处：
   * `%TEMP%\fast-launch.log`：应当出现
     `类加载 hook 已打开` → `ClassFileLoadHook：已提交改写后的 PatchEngine` → `[bridge] 缓存已载入` →
     **`[bridge] 命中缓存：<包名> → N 个类（跳过 ClassGraph 扫描）`**（这条是"真的加速了"的唯一直接证据）；
   * `%USERPROFILE%\Zomboid\Logs\<最新>\…_DebugLog.txt`：模组包的 `[ZB] Scanned N classes in package X`
     应当**消失**，启动耗时随之下降。
5. 想立刻验证"兜底不会弄坏游戏"：设环境变量 `PZM_FAST_LAUNCH_SKIP_BRIDGE=1` 再启动一次，
   游戏应当照常可玩（只是没有加速）。
6. 想立刻让功能闭嘴：设 `PZM_FAST_LAUNCH_DISABLE=1`，或在设置里关掉开关（关掉会当场清理启动文件）。
