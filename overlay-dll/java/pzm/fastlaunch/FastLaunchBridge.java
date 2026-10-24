package pzm.fastlaunch;

import java.io.IOException;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * 极速启动的桥接类（由启动器注入，见 docs/architecture.md §6.3）。
 *
 * <p>改写器把 {@code PatchEngine.collectPatches(String, ClassLoader)} 的**方法体**换成
 * {@code invokestatic FastLaunchBridge.collect(...)}，所以本类方法签名必须与之一致。
 *
 * <p>回退设计：改写器**不会**丢弃原实现——它把原方法改名为 {@code collectPatches$pzm}
 * （名称是常量池里的一个 Utf8，改名前会新增独立条目，因此 ZombieBuddy 的调用点仍指向
 * {@code collectPatches} = 我们这段新方法体），需要回退时用反射把原实现调回来，
 * 回退到的是**真正的原生实现**，而不是"我们复刻一遍原逻辑"。
 *
 * <p><b>当前默认是"强制模式"（不回退）</b>：只用启动器预扫描数据，缓存里没有的包
 * 返回空列表、绝不调用原生实现。要恢复旧的回退行为，把环境变量
 * {@code PZM_FAST_LAUNCH_ALLOW_FALLBACK} 设成 {@code 1}。
 *
 * <p>本类刻意不依赖任何第三方库（ZombieBuddy 的 jar 里有什么不能假定）。
 */
public final class FastLaunchBridge {

    /** 缓存文件名，必须与启动器侧一致。 */
    private static final String CACHE_FILE = "zb-prescan-cache.json";
    /** 期望的缓存格式版本，必须与启动器侧 FAST_LAUNCH_CACHE_FORMAT 一致。 */
    private static final int CACHE_FORMAT = 1;
    /** 原实现被改到的名字。 */
    private static final String ORIGINAL_METHOD = "collectPatches$pzm";
    /** 目标类。 */
    private static final String PATCH_ENGINE = "me.zed_0xff.zombie_buddy.PatchEngine";

    /** 已解析的缓存：basePackage -> 该包下的 @Patch 类名。解析只做一次。 */
    private static volatile Map<String, List<String>> cacheByPackage;
    /** 缓存是否可用（格式/指纹检查通过）。 */
    private static volatile boolean cacheUsable;
    /** 原实现的 Method 句柄，懒加载。 */
    private static volatile Method originalMethod;
    /** 是否允许回退到原实现；null = 还没读过环境变量。 */
    private static volatile Boolean allowFallback;
    /** "调用方 loader 不可用、改用系统加载器"这条日志只记一次。 */
    private static volatile boolean loaderFallbackLogged;
    /** "已提前加载目标类"这条日志只记一次。 */
    private static volatile boolean preloadLogged;

    /**
     * 强制模式的逃生开关。
     *
     * <p>默认**关闭回退**：只用启动器预扫描数据，缓存里没有的包一律返回空列表，
     * 绝不调用 ZombieBuddy 的原生 ClassGraph 扫描。把它设成 {@code 1}
     * （要能传进游戏进程，例如设成系统环境变量后重启 Steam）就恢复旧的
     * "未命中就回退"行为。
     */
    private static final String ALLOW_FALLBACK_ENV = "PZM_FAST_LAUNCH_ALLOW_FALLBACK";

    /**
     * **实验开关**：提前把 {@code @Patch(className=...)} 指向的游戏类加载进来。
     *
     * <p>为什么需要（2026-10-15 崩溃排查）：ZombieBuddy 的打补丁流程依赖"目标类已经加载"。
     * 原生扫描要跑几十秒，这期间游戏自己的线程已经把很多类加载好了，所以 ZB 日志里是
     * `(retransformed)`；极速启动把补丁清单瞬间返回，ZB 就会撞上还没加载的
     * `zombie.vehicles.UI3DScene`，走 ByteBuddy 的 `(new load)` 路径，紧接着游戏自己的
     * `LuaManager$Exposer.exposeAll` 再 loadClass 同一个类 →
     * `LinkageError: loader 'app' attempted duplicate class definition`。
     *
     * <p>打开后我们在 `collect()` 里（此时 ZB 的 ByteBuddy agent 还没 installOn）先把目标类
     * 按系统加载器加载进来，让 ZB 改走 retransform 路径。环境变量设成 `1`、
     * 或在 `%TEMP%` 建一个 `pzm-fast-launch-preload.flag` 即生效。
     */
    private static final String PRELOAD_TARGETS_ENV = "PZM_FAST_LAUNCH_PRELOAD_TARGETS";
    /** 上面那个开关的"文件版"，方便不想设环境变量时用。 */
    private static final String PRELOAD_TARGETS_MARKER = "pzm-fast-launch-preload.flag";
    /** ZombieBuddy 的 @Patch 注解全名。 */
    private static final String PATCH_ANNOTATION = "me.zed_0xff.zombie_buddy.Patch";

    private FastLaunchBridge() {
    }

    /** 读一次逃生开关并记一行模式日志。 */
    private static boolean allowFallback() {
        Boolean cached = allowFallback;
        if (cached == null) {
            boolean allowed = "1".equals(System.getenv(ALLOW_FALLBACK_ENV));
            allowFallback = Boolean.valueOf(allowed);
            log(allowed
                    ? "回退模式：未命中/异常时调用 ZombieBuddy 原生 collectPatches（"
                            + ALLOW_FALLBACK_ENV + "=1）"
                    : "强制模式：只用启动器预扫描数据，未命中返回空列表、绝不回退（"
                            + ALLOW_FALLBACK_ENV + " 未设为 1）");
            return allowed;
        }
        return cached.booleanValue();
    }

    /**
     * 与 {@code PatchEngine.collectPatches(String, ClassLoader)} 同签名。
     *
     * <p>默认强制使用启动器数据：命中缓存就用缓存类名取回类；缓存里没有这个包
     * （或缓存本身不可用）就返回空列表，**不调用**原实现。只有把
     * {@link #ALLOW_FALLBACK_ENV} 设成 {@code 1} 时才恢复"回退 ZombieBuddy 原生扫描"。
     *
     * <p>缓存命中时，个别类名取不到（例如模组更新后类被删）只跳过那一个并记日志，
     * 仍然不回退——否则一个坏类名会把整包都推回 ClassGraph。
     */
    public static List<Class<?>> collect(String packageName, ClassLoader loader) {
        boolean fallbackAllowed = allowFallback();

        List<String> classNames = null;
        try {
            classNames = lookup(packageName);
        } catch (Throwable failure) {
            log("读缓存失败" + (fallbackAllowed ? "，回退原生扫描" : "，强制模式下返回空列表")
                    + ": " + failure);
        }

        if (classNames != null) {
            List<Class<?>> classes = new ArrayList<Class<?>>(classNames.size());
            for (String className : classNames) {
                try {
                    // initialize=false：与 ClassGraph 的行为一致，不提前初始化 @Patch 类。
                    classes.add(resolveClass(className, loader));
                } catch (Throwable failure) {
                    log("类名取不到，跳过（不回退整包）：" + className + " → " + failure);
                }
            }
            log("命中缓存：" + packageName + " → " + classes.size() + "/" + classNames.size()
                    + " 个类（跳过 ClassGraph 扫描）");
            if (preloadTargetsEnabled()) {
                preloadPatchTargets(classes);
            }
            return classes;
        }

        if (!fallbackAllowed) {
            // 强制模式的核心：查不到就当作"这个包没有补丁"，绝不调原实现。
            log("强制模式（不回退）：缓存里没有这个包或缓存不可用，返回空列表：" + packageName);
            return new ArrayList<Class<?>>(0);
        }

        return callOriginal(packageName, loader);
    }

    /**
     * 把缓存里的类名转成 {@link Class#forName(String, boolean, ClassLoader)} 认的形式。
     *
     * <p>踩过的坑（2026-10-15）：启动器扫描时记录的是**内部名**（`viewpoint/Patch_AimAxis`，
     * 斜杠分隔），直接喂给 `Class.forName` 会全部抛 `ClassNotFoundException` —— 表现就是
     * "每个模组都回退原生扫描、一点没加速"，而日志里只看到一串 CNFE。
     * 这里统一把 `/` 换成 `.`，两种形式（内部名 / 二进制名）都能用。
     */
    private static String binaryName(String className) {
        return className.indexOf('/') < 0 ? className : className.replace('/', '.');
    }

    /** 取二进制类名所在的包（内部类用 `$`，所以按最后一个 `.` 切）。 */
    private static String packageOf(String binaryName) {
        int dot = binaryName.lastIndexOf('.');
        return dot <= 0 ? null : binaryName.substring(0, dot);
    }

    /**
     * 取出一个 @Patch 类。
     *
     * <p><b>关键（2026-10-15 实测）</b>：ZombieBuddy 的 `Loader.loadJar()` 调的是
     * `ApplyPatchesFromPackage(package, **null**, phase)` —— 传进来的 `loader` 就是 `null`！
     * 之前直接 `Class.forName(name, false, loader)` 会退化成 bootstrap 加载器，
     * 于是**每一个类**都 `ClassNotFoundException`（日志里 43/43 全部"类名取不到"），
     * ZB 自己的补丁一个都装不上 → 游戏里弹"ZombieBuddy 未安装"。
     *
     * <p>ZB 原生走的是 `Instrumentation.appendToSystemClassLoaderSearch(jar)` + ClassGraph
     * `overrideClasspath`，用的正是**系统加载器**。所以这里按
     * `调用方 loader → 系统加载器 → 线程上下文加载器` 依次尝试，和原生行为对齐。
     */
    private static Class<?> resolveClass(String className, ClassLoader loader)
            throws ClassNotFoundException {
        String binary = binaryName(className);
        ClassNotFoundException failure = null;
        for (ClassLoader candidate : candidateLoaders(loader)) {
            try {
                Class<?> resolved = Class.forName(binary, false, candidate);
                if (candidate != loader && !loaderFallbackLogged) {
                    loaderFallbackLogged = true;
                    log("调用方给的加载器" + (loader == null ? "是 null" : "取不到类")
                            + "，改用系统/上下文加载器取类（与 ZombieBuddy 原生一致）");
                }
                return resolved;
            } catch (ClassNotFoundException missing) {
                failure = missing;
            }
        }
        if (failure != null) {
            throw failure;
        }
        throw new ClassNotFoundException(binary);
    }

    /** 实验开关是否打开（环境变量 `=1` 或 `%TEMP%` 下有标记文件）。 */
    private static boolean preloadTargetsEnabled() {
        if ("1".equals(System.getenv(PRELOAD_TARGETS_ENV))) {
            return true;
        }
        String temp = System.getenv("TEMP");
        return temp != null
                && Files.isRegularFile(Paths.get(temp, PRELOAD_TARGETS_MARKER));
    }

    /**
     * 把每个 @Patch 类声明的目标类先加载进来（`initialize=false`），
     * 让 ZombieBuddy 后面看到"已加载"而不是"新加载"。
     * 任何失败都忽略——这只是为了让 ZB 走 retransform 路径，不影响我们返回补丁类。
     */
    private static void preloadPatchTargets(List<Class<?>> patchClasses) {
        int loaded = 0;
        for (Class<?> patchClass : patchClasses) {
            try {
                for (java.lang.annotation.Annotation annotation : patchClass.getAnnotations()) {
                    if (!annotation.annotationType().getName().equals(PATCH_ANNOTATION)) {
                        continue;
                    }
                    Object target = annotation.annotationType()
                            .getMethod("className")
                            .invoke(annotation);
                    if (target instanceof String && !((String) target).isEmpty()) {
                        Class.forName((String) target, false, ClassLoader.getSystemClassLoader());
                        loaded++;
                    }
                }
            } catch (Throwable ignored) {
                // 目标类加载失败不影响补丁类本身；ZB 自己会处理。
            }
        }
        if (loaded > 0 && !preloadLogged) {
            preloadLogged = true;
            log("已提前加载 " + loaded + " 个 @Patch 目标类（走 retransform 而不是 new load）");
        }
    }

    /** 尝试顺序：调用方给的 loader → 系统加载器 → 线程上下文加载器（去重、跳过 null）。 */
    private static List<ClassLoader> candidateLoaders(ClassLoader loader) {
        List<ClassLoader> loaders = new ArrayList<ClassLoader>(3);
        if (loader != null) {
            loaders.add(loader);
        }
        ClassLoader system = ClassLoader.getSystemClassLoader();
        if (system != null && !loaders.contains(system)) {
            loaders.add(system);
        }
        ClassLoader context = Thread.currentThread().getContextClassLoader();
        if (context != null && !loaders.contains(context)) {
            loaders.add(context);
        }
        return loaders;
    }

    /** 用反射调用被改名的原实现。 */
    private static List<Class<?>> callOriginal(String packageName, ClassLoader loader) {
        try {
            Method method = originalMethod;
            if (method == null) {
                Class<?> engine = Class.forName(PATCH_ENGINE, false, loader);
                method = engine.getDeclaredMethod(
                        ORIGINAL_METHOD, String.class, ClassLoader.class);
                method.setAccessible(true);
                originalMethod = method;
            }
            @SuppressWarnings("unchecked")
            List<Class<?>> result = (List<Class<?>>) method.invoke(null, packageName, loader);
            return result;
        } catch (InvocationTargetException wrapped) {
            Throwable cause = wrapped.getCause();
            if (cause instanceof RuntimeException) {
                throw (RuntimeException) cause;
            }
            if (cause instanceof Error) {
                throw (Error) cause;
            }
            throw new IllegalStateException("原 collectPatches 抛出受检异常", cause);
        } catch (ReflectiveOperationException failure) {
            throw new IllegalStateException("拿不到原 collectPatches 实现", failure);
        }
    }

    /**
     * 查缓存里这个包（**及其子包**）的类名；缓存不可用或没有任何命中都返回 null。
     *
     * <p>为什么必须带子包：ZombieBuddy 是按 jar 声明的包名调 {@code collectPatches} 的，
     * 而它自己的补丁类在 {@code me.zed_0xff.zombie_buddy.patches}（声明的包是父包）——
     * ClassGraph 的 {@code acceptPackages(pkg)} 本来就会把子包算进去，这里要跟它一致。
     * 只做精确匹配会把这些补丁整批丢掉。
     */
    private static List<String> lookup(String packageName) throws IOException {
        if (!cacheUsable && !loadCache()) {
            return null;
        }
        String prefix = packageName + ".";
        List<String> classes = new ArrayList<String>();
        boolean found = false;
        for (Map.Entry<String, List<String>> entry : cacheByPackage.entrySet()) {
            String cachedPackage = entry.getKey();
            if (cachedPackage.equals(packageName) || cachedPackage.startsWith(prefix)) {
                found = true;
                classes.addAll(entry.getValue());
            }
        }
        if (!found) {
            // 也要记一行：否则"没这个包（正常）"和"包名形式对不上（bug）"在日志里长得一样，
            // 2026-10-15 就是靠这类沉默的回退才误判成"缓存路径没问题"。
            // 这里只报"没命中"，回退还是强制由 collect() 按模式决定并另行记一行。
            log("缓存无此包及其子包：" + packageName);
        }
        return found ? classes : null;
    }

    /** 只解析一次缓存；解析失败就把缓存标记为不可用，之后永远走原实现。 */
    private static boolean loadCache() {
        synchronized (FastLaunchBridge.class) {
            if (cacheByPackage != null) {
                return cacheUsable;
            }
            cacheByPackage = new HashMap<String, List<String>>();
            try {
                Path path = cachePath();
                if (path == null || !Files.isRegularFile(path)) {
                    log("没有预扫描缓存");
                    return false;
                }
                String content = new String(Files.readAllBytes(path), StandardCharsets.UTF_8);
                if (jsonNumber(content, "format") != CACHE_FORMAT) {
                    log("缓存格式版本不匹配");
                    return false;
                }
                if (jsonString(content, "modSetHash") == null) {
                    log("缓存缺少指纹");
                    return false;
                }
                for (String entry : jsonObjects(content, "mods")) {
                    // 按**类自己的包**建索引，而不是模组声明的 basePackage：
                    // ZombieBuddy 自己的补丁在 me.zed_0xff.zombie_buddy.patches，
                    // 而 mod.info 声明的包是父包 me.zed_0xff.zombie_buddy ——
                    // 按 basePackage 建索引会让这个包永远查不到（进游戏报 ZB 未安装）。
                    for (String className : jsonStrings(entry, "patchClasses")) {
                        String binary = binaryName(className);
                        String packageName = packageOf(binary);
                        if (packageName == null) {
                            continue;
                        }
                        List<String> classes = cacheByPackage.get(packageName);
                        if (classes == null) {
                            classes = new ArrayList<String>();
                            cacheByPackage.put(packageName, classes);
                        }
                        classes.add(binary);
                    }
                }
                cacheUsable = true;
                log("缓存已载入：包数=" + cacheByPackage.size());
                return true;
            } catch (Throwable failure) {
                log("解析缓存失败: " + failure);
                return false;
            }
        }
    }

    /** `%USERPROFILE%\Zomboid\.pzmanager\<CACHE_FILE>`。 */
    private static Path cachePath() {
        String profile = System.getenv("USERPROFILE");
        if (profile == null || profile.isEmpty()) {
            return null;
        }
        return Paths.get(profile, "Zomboid", ".pzmanager", CACHE_FILE);
    }

    /**
     * 从 JSON 文本里取一个字符串字段。
     *
     * <p>刻意手写而不引 JSON 库：辅助类被注入到游戏进程里，能不依赖外部库就不依赖，
     * 少一个"运行时找不到类"的失败面；解析不出来就返回 null，调用方一律回退原实现。
     */
    private static String jsonString(String json, String key) {
        String needle = "\"" + key + "\"";
        int at = json.indexOf(needle);
        if (at < 0) {
            return null;
        }
        int colon = json.indexOf(':', at + needle.length());
        if (colon < 0) {
            return null;
        }
        int quote = json.indexOf('"', colon + 1);
        if (quote < 0) {
            return null;
        }
        int end = json.indexOf('"', quote + 1);
        if (end < 0) {
            return null;
        }
        return json.substring(quote + 1, end);
    }

    /** 取一个数字字段；取不到返回 -1。 */
    private static long jsonNumber(String json, String key) {
        String needle = "\"" + key + "\"";
        int at = json.indexOf(needle);
        if (at < 0) {
            return -1;
        }
        int colon = json.indexOf(':', at + needle.length());
        if (colon < 0) {
            return -1;
        }
        int index = colon + 1;
        while (index < json.length() && Character.isWhitespace(json.charAt(index))) {
            index++;
        }
        int start = index;
        while (index < json.length() && Character.isDigit(json.charAt(index))) {
            index++;
        }
        if (index == start) {
            return -1;
        }
        try {
            return Long.parseLong(json.substring(start, index));
        } catch (NumberFormatException failure) {
            return -1;
        }
    }

    /** 取数组字段里的所有字符串元素。 */
    private static List<String> jsonStrings(String json, String key) {
        List<String> values = new ArrayList<String>();
        String array = jsonArray(json, key);
        if (array == null) {
            return values;
        }
        int index = 0;
        while (true) {
            int quote = array.indexOf('"', index);
            if (quote < 0) {
                return values;
            }
            int end = array.indexOf('"', quote + 1);
            if (end < 0) {
                return values;
            }
            values.add(array.substring(quote + 1, end));
            index = end + 1;
        }
    }

    /** 取数组字段里所有对象（按最外层花括号配平切分）。 */
    private static List<String> jsonObjects(String json, String key) {
        List<String> objects = new ArrayList<String>();
        String array = jsonArray(json, key);
        if (array == null) {
            return objects;
        }
        int depth = 0;
        int start = -1;
        for (int index = 0; index < array.length(); index++) {
            char current = array.charAt(index);
            if (current == '{') {
                if (depth == 0) {
                    start = index;
                }
                depth++;
            } else if (current == '}') {
                depth--;
                if (depth == 0 && start >= 0) {
                    objects.add(array.substring(start, index + 1));
                    start = -1;
                }
            }
        }
        return objects;
    }

    /** 取形如 `"key": [ ... ]` 的方括号内容（配平切分）。 */
    private static String jsonArray(String json, String key) {
        String needle = "\"" + key + "\"";
        int at = json.indexOf(needle);
        if (at < 0) {
            return null;
        }
        int open = json.indexOf('[', at + needle.length());
        if (open < 0) {
            return null;
        }
        int depth = 0;
        for (int index = open; index < json.length(); index++) {
            char current = json.charAt(index);
            if (current == '[') {
                depth++;
            } else if (current == ']') {
                depth--;
                if (depth == 0) {
                    return json.substring(open + 1, index);
                }
            }
        }
        return null;
    }

    /** 写日志到 `%TEMP%\fast-launch.log`（与注入器同一个文件，失败就吞掉）。 */
    private static void log(String message) {
        try {
            String temp = System.getenv("TEMP");
            if (temp == null) {
                return;
            }
            Path log = Paths.get(temp, "fast-launch.log");
            String line = "[bridge] " + message + System.lineSeparator();
            Files.write(log, line.getBytes(StandardCharsets.UTF_8),
                    java.nio.file.StandardOpenOption.CREATE,
                    java.nio.file.StandardOpenOption.APPEND);
        } catch (Throwable ignored) {
            // 日志失败绝不能影响游戏。
        }
    }
}
