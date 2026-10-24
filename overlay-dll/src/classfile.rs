//! 最小 class 文件读取器：只为"接管 `PatchEngine.collectPatches`"这一件事服务。
//!
//! 为什么要它：`collectPatches` 是静态方法、`Loader` 又没有保存 `@Patch` 清单的字段
//! （见 docs/architecture.md §6.3 的设计结论），所以短路只能落到字节码层面，
//! 而改写字节码就必须先能读懂 class 文件——常量池是第一步。
//!
//! 设计约定：
//! * 只读，不猜；任何越界/非法标签都返回 `Err`，由调用方放弃缓存、回退原生扫描；
//! * 不认识的常量池标签按原样保留（`PoolEntry::Other`），改写时不碰它们；
//! * 本模块不依赖任何 crate（注入器是 cdylib，能不加依赖就不加）。

/// 常量池标签（JVMS §4.4）。只列会用到的；其余走 `Other`。
const CONSTANT_UTF8: u8 = 1;
const CONSTANT_INTEGER: u8 = 3;
const CONSTANT_FLOAT: u8 = 4;
const CONSTANT_LONG: u8 = 5;
const CONSTANT_DOUBLE: u8 = 6;
const CONSTANT_CLASS: u8 = 7;
const CONSTANT_STRING: u8 = 8;
const CONSTANT_FIELDREF: u8 = 9;
const CONSTANT_METHODREF: u8 = 10;
const CONSTANT_INTERFACE_METHODREF: u8 = 11;
const CONSTANT_NAME_AND_TYPE: u8 = 12;
const CONSTANT_METHOD_HANDLE: u8 = 15;
const CONSTANT_METHOD_TYPE: u8 = 16;
const CONSTANT_DYNAMIC: u8 = 17;
const CONSTANT_INVOKE_DYNAMIC: u8 = 18;
const CONSTANT_MODULE: u8 = 19;
const CONSTANT_PACKAGE: u8 = 20;

/// class 文件幻数 `0xCAFEBABE`。
const MAGIC: u32 = 0xCAFE_BABE;
/// class 文件头长度：幻数 + minor + major + 常量池计数。
const HEADER_LENGTH: usize = 10;

/// 顺序读取器：所有越界都在这里收敛成 `Err`，避免解析代码到处判边界。
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn u1(&mut self) -> Result<u8, String> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| "class 文件提前结束（u1）".to_string())?;
        self.offset += 1;
        Ok(value)
    }

    fn u2(&mut self) -> Result<u16, String> {
        let high = u16::from(self.u1()?);
        let low = u16::from(self.u1()?);
        Ok((high << 8) | low)
    }

    fn u4(&mut self) -> Result<u32, String> {
        let high = u32::from(self.u2()?);
        let low = u32::from(self.u2()?);
        Ok((high << 16) | low)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| "class 文件长度溢出".to_string())?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "class 文件提前结束（take）".to_string())?;
        self.offset = end;
        Ok(slice)
    }

    /// 直接定位到某个偏移（用于"先走一遍拿边界、再回到边界继续解析"）。
    fn seek(&mut self, offset: usize) -> Result<(), String> {
        if offset > self.bytes.len() {
            return Err("定位越界".to_string());
        }
        self.offset = offset;
        Ok(())
    }
}

/// 常量池条目。`Other` 保留原始字节（含标签），改写时原样写回。
#[derive(Debug, Clone, PartialEq, Eq)]
enum PoolEntry {
    Utf8(String),
    Class(u16),
    String(u16),
    NameAndType { name: u16, descriptor: u16 },
    Other { tag: u8, payload: Vec<u8> },
}

/// 解析结果：目前只需要"版本 + 常量池"，方法表留给下一步。
#[derive(Debug, Clone)]
struct ClassFile {
    major: u16,
    pool: Vec<Option<PoolEntry>>,
}

impl ClassFile {
    /// 解析 class 文件头部与常量池（不进入字段/方法表）。
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = Reader::new(bytes);
        let magic = reader.u4()?;
        if magic != MAGIC {
            return Err(format!("不是 class 文件：幻数 0x{magic:08x}"));
        }
        let _minor = reader.u2()?;
        let major = reader.u2()?;
        let count = reader.u2()?;
        if count == 0 {
            return Err("class 文件常量池计数为 0".to_string());
        }

        // 索引 0 不使用，所以容量是 count，实际从 1 开始填。
        let mut pool: Vec<Option<PoolEntry>> = vec![None; usize::from(count)];
        let mut index = 1usize;
        while index < usize::from(count) {
            let tag = reader.u1()?;
            let entry = match tag {
                CONSTANT_UTF8 => {
                    let length = usize::from(reader.u2()?);
                    let raw = reader.take(length)?;
                    // class 文件里的 Utf8 是"改进版 UTF-8"，我们只按 UTF-8 尽力解码：
                    // 解不出来就整体放弃（宁可回退，也不要写坏字节码）。
                    let text = std::str::from_utf8(raw)
                        .map_err(|_| "常量池 Utf8 不是合法 UTF-8".to_string())?
                        .to_string();
                    PoolEntry::Utf8(text)
                }
                CONSTANT_CLASS => PoolEntry::Class(reader.u2()?),
                CONSTANT_STRING => PoolEntry::String(reader.u2()?),
                CONSTANT_NAME_AND_TYPE => PoolEntry::NameAndType {
                    name: reader.u2()?,
                    descriptor: reader.u2()?,
                },
                CONSTANT_INTEGER | CONSTANT_FLOAT => {
                    let payload = reader.take(4)?.to_vec();
                    PoolEntry::Other { tag, payload }
                }
                CONSTANT_LONG | CONSTANT_DOUBLE => {
                    let payload = reader.take(8)?.to_vec();
                    pool[index] = Some(PoolEntry::Other { tag, payload });
                    // long/double 占两个常量池槽位（JVMS §4.4.5）。
                    index += 1;
                    if index >= usize::from(count) {
                        return Err("常量池计数与 long/double 槽位不一致".to_string());
                    }
                    pool[index] = None;
                    index += 1;
                    continue;
                }
                CONSTANT_FIELDREF | CONSTANT_METHODREF | CONSTANT_INTERFACE_METHODREF => {
                    let payload = reader.take(4)?.to_vec();
                    PoolEntry::Other { tag, payload }
                }
                CONSTANT_METHOD_HANDLE => {
                    let payload = reader.take(3)?.to_vec();
                    PoolEntry::Other { tag, payload }
                }
                CONSTANT_METHOD_TYPE | CONSTANT_MODULE | CONSTANT_PACKAGE => {
                    let payload = reader.take(2)?.to_vec();
                    PoolEntry::Other { tag, payload }
                }
                CONSTANT_DYNAMIC | CONSTANT_INVOKE_DYNAMIC => {
                    let payload = reader.take(4)?.to_vec();
                    PoolEntry::Other { tag, payload }
                }
                other => return Err(format!("不认识的常量池标签 {other}")),
            };
            pool[index] = Some(entry);
            index += 1;
        }

        Ok(Self { major, pool })
    }

    /// 按索引取 Utf8 文本（越界或类型不对都返回 None）。
    fn utf8(&self, index: u16) -> Option<&str> {
        match self.pool.get(usize::from(index))? {
            Some(PoolEntry::Utf8(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    /// 常量池里出现的所有 Utf8 文本，便于测试与调试。
    fn utf8_entries(&self) -> Vec<&str> {
        self.pool
            .iter()
            .flatten()
            .filter_map(|entry| match entry {
                PoolEntry::Utf8(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// 方法在原始字节里的 `Code` 区间（`code_offset..code_offset+code_length`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CodeRange {
    offset: usize,
    length: usize,
}

/// 方法表条目：改写时要靠"名字 + 描述符"精确定位，所以两者都留在常量池索引上。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodInfo {
    name: String,
    descriptor: String,
    code: Option<CodeRange>,
}

/// 跳过 `attributes_count` 个属性（每个：name_index u2 + length u4 + length 字节）。
fn skip_attributes(reader: &mut Reader<'_>) -> Result<(), String> {
    let count = reader.u2()?;
    for _ in 0..count {
        let _name_index = reader.u2()?;
        let length = reader.u4()?;
        reader.take(usize::try_from(length).map_err(|_| "属性长度过大".to_string())?)?;
    }
    Ok(())
}

/// 解析方法表（接在常量池之后：access/this/super/interfaces/fields 依次跳过）。
///
/// 只收集我们关心的东西：方法名、描述符、以及 `Code` 属性指向的字节码区间。
/// 任何结构异常都返回 `Err`，由调用方放弃缓存。
fn parse_methods(bytes: &[u8], pool: &[Option<PoolEntry>]) -> Result<Vec<MethodInfo>, String> {
    let utf8 = |index: u16| -> String {
        match pool.get(usize::from(index)) {
            Some(Some(PoolEntry::Utf8(text))) => text.clone(),
            _ => String::new(),
        }
    };

    let mut reader = Reader::new(bytes);
    reader.take(HEADER_LENGTH)?; // 幻数 + minor + major + 常量池计数
    // 重新走一遍常量池，把读取位置推到常量池之后。
    let count = {
        let mut probe = Reader::new(bytes);
        probe.take(8)?;
        probe.u2()?
    };
    let mut index = 1usize;
    while index < usize::from(count) {
        let tag = reader.u1()?;
        match tag {
            CONSTANT_UTF8 => {
                let length = usize::from(reader.u2()?);
                reader.take(length)?;
            }
            CONSTANT_CLASS | CONSTANT_STRING | CONSTANT_METHOD_TYPE | CONSTANT_MODULE
            | CONSTANT_PACKAGE => {
                reader.take(2)?;
            }
            CONSTANT_INTEGER | CONSTANT_FLOAT | CONSTANT_FIELDREF | CONSTANT_METHODREF
            | CONSTANT_INTERFACE_METHODREF | CONSTANT_NAME_AND_TYPE | CONSTANT_DYNAMIC
            | CONSTANT_INVOKE_DYNAMIC => {
                reader.take(4)?;
            }
            CONSTANT_METHOD_HANDLE => {
                reader.take(3)?;
            }
            CONSTANT_LONG | CONSTANT_DOUBLE => {
                reader.take(8)?;
                index += 2;
                continue;
            }
            other => return Err(format!("不认识的常量池标签 {other}")),
        }
        index += 1;
    }

    let _access_flags = reader.u2()?;
    let _this_class = reader.u2()?;
    let _super_class = reader.u2()?;
    let interfaces = reader.u2()?;
    reader.take(usize::from(interfaces) * 2)?;

    let fields = reader.u2()?;
    for _ in 0..fields {
        reader.take(6)?; // access_flags + name_index + descriptor_index
        skip_attributes(&mut reader)?;
    }

    let method_count = reader.u2()?;
    let mut methods = Vec::with_capacity(usize::from(method_count));
    for _ in 0..method_count {
        let _access = reader.u2()?;
        let name = utf8(reader.u2()?);
        let descriptor = utf8(reader.u2()?);
        let attribute_count = reader.u2()?;
        let mut code = None;
        for _ in 0..attribute_count {
            let attribute_name = utf8(reader.u2()?);
            let length = usize::try_from(reader.u4()?).map_err(|_| "属性长度过大".to_string())?;
            if attribute_name == "Code" {
                let _max_stack = reader.u2()?;
                let _max_locals = reader.u2()?;
                let code_length =
                    usize::try_from(reader.u4()?).map_err(|_| "字节码长度过大".to_string())?;
                let offset = reader.offset + 0;
                reader.take(code_length)?;
                code = Some(CodeRange {
                    offset,
                    length: code_length,
                });
                // Code 属性后面还有异常表/子属性，按剩余长度跳过。
                let consumed = 8 + code_length;
                reader.take(length.saturating_sub(consumed))?;
            } else {
                reader.take(length)?;
            }
        }
        methods.push(MethodInfo {
            name,
            descriptor,
            code,
        });
    }
    Ok(methods)
}

/// 常量池增量：要拼接到原池子后面的字节 + 占用的槽位数。
#[derive(Debug, Clone)]
struct PoolAddition {
    bytes: Vec<u8>,
    slots: u16,
}

/// 编码一个 `CONSTANT_Utf8`。
fn encode_utf8(text: &str) -> Result<Vec<u8>, String> {
    let length = u16::try_from(text.len()).map_err(|_| "Utf8 常量过长".to_string())?;
    let mut bytes = Vec::with_capacity(3 + text.len());
    bytes.push(CONSTANT_UTF8);
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(bytes)
}

/// 编码一个 `CONSTANT_Class`（指向 `name_index`）。
fn encode_class(name_index: u16) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(3);
    bytes.push(CONSTANT_CLASS);
    bytes.extend_from_slice(&name_index.to_be_bytes());
    bytes
}

/// 编码一个 `CONSTANT_NameAndType`。
fn encode_name_and_type(name_index: u16, descriptor_index: u16) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(5);
    bytes.push(CONSTANT_NAME_AND_TYPE);
    bytes.extend_from_slice(&name_index.to_be_bytes());
    bytes.extend_from_slice(&descriptor_index.to_be_bytes());
    bytes
}

/// 编码一个 `CONSTANT_Methodref`。
fn encode_methodref(class_index: u16, name_and_type_index: u16) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(5);
    bytes.push(CONSTANT_METHODREF);
    bytes.extend_from_slice(&class_index.to_be_bytes());
    bytes.extend_from_slice(&name_and_type_index.to_be_bytes());
    bytes
}

/// 生成"调用一个静态辅助方法"所需的全部常量池增量。
///
/// `pool_count` 是**原**常量池计数（含未使用的 0 号槽，即新条目的起始索引就是它）。
/// 返回 (增量, 新计数, Methodref 索引)，调用方把增量拼到原池子后面、把计数改成新值即可。
///
/// 刻意不做 Utf8 去重：重复的 Utf8 在 class 文件里完全合法，只是略浪费；
/// 少一段去重逻辑就少一处可能写坏字节码的地方。
fn encode_static_call_reference(
    pool_count: u16,
    class_name: &str,
    method_name: &str,
    descriptor: &str,
) -> Result<(PoolAddition, u16, u16), String> {
    let mut next = pool_count;
    let mut bytes = Vec::new();

    let class_name_index = next;
    bytes.extend_from_slice(&encode_utf8(class_name)?);
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    let class_index = next;
    bytes.extend_from_slice(&encode_class(class_name_index));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    let method_name_index = next;
    bytes.extend_from_slice(&encode_utf8(method_name)?);
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    let descriptor_index = next;
    bytes.extend_from_slice(&encode_utf8(descriptor)?);
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    let name_and_type_index = next;
    bytes.extend_from_slice(&encode_name_and_type(method_name_index, descriptor_index));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    let methodref_index = next;
    bytes.extend_from_slice(&encode_methodref(class_index, name_and_type_index));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    Ok((
        PoolAddition {
            bytes,
            slots: next - pool_count,
        },
        next,
        methodref_index,
    ))
}

/// 生成"短路 `collectPatches`"的替换字节码。
///
/// 目标方法签名是 `(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;`，
/// 我们**只替换方法体、不动签名**，所以参数已经在局部变量 0、1 里：
///
/// ```text
/// aload_0                        // packageName
/// aload_1                        // classLoader
/// invokestatic <helper>          // 辅助类自己去找缓存文件（走 System.getenv，不必加参数）
/// areturn
/// ```
///
/// 共 6 字节；`max_stack` 至少要是 2（压入两个引用）。缓存路径**不作为参数**传入：
/// 辅助类自己按 `%USERPROFILE%` 定位，这样替换后的调用点与原签名完全一致，
/// 不需要额外生成 `ldc`，也就少一处可能写坏字节码的地方。
fn encode_short_circuit_body(methodref_index: u16) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6);
    bytes.push(0x2A); // aload_0
    bytes.push(0x2B); // aload_1
    bytes.push(0xB8); // invokestatic
    bytes.extend_from_slice(&methodref_index.to_be_bytes());
    bytes.push(0xB0); // areturn
    bytes
}

/// 包装方法的字节码布局：先问辅助类，**任何 Throwable 都回退到原实现**。
///
/// ```text
///   0: aload_0
///   1: aload_1
///   2: invokestatic <辅助类>.collect
///   5: areturn
///   6: pop                       ← 异常处理器入口：丢掉 Throwable
///   7: aload_0
///   8: aload_1
///   9: invokestatic <本类>.collectPatches$pzm
///  12: areturn
/// ```
///
/// 为什么必须带这层兜底：类加载 hook 现在是在 `Agent_OnLoad` 里就打开的（必须赶在
/// ZombieBuddy 自己的 agent 定义 `PatchEngine` 之前），而那一刻**还没法把辅助类送进 JVM**
/// （onload 阶段拿不到 `JNIEnv`，只能等 `VMInit` 回调）。如果包装体没有兜底，辅助类缺席
/// 会让 ZombieBuddy 的 `collectPatches` 抛 `NoClassDefFoundError` —— 那是把加载器弄坏，
/// 不是"回退原生扫描"。有了兜底：辅助类到位之前的所有调用都直接走 ZombieBuddy 的原实现。
#[derive(Debug, Clone, PartialEq, Eq)]
struct WrapperBody {
    code: Vec<u8>,
    exception_table: Vec<u8>,
    stack_map: Vec<u8>,
}

/// 异常处理器入口偏移（= `pop` 的位置，也是受保护区间的结束偏移）。
const WRAPPER_HANDLER_OFFSET: u16 = 6;
/// `StackMapTable` 里 `Object_variable_info` 的 tag。
const ITEM_OBJECT: u8 = 7;

fn encode_wrapper_body(
    bridge_methodref: u16,
    renamed_methodref: u16,
    string_class: u16,
    loader_class: u16,
    throwable_class: u16,
) -> WrapperBody {
    let mut code = Vec::with_capacity(13);
    code.push(0x2A); // aload_0
    code.push(0x2B); // aload_1
    code.push(0xB8); // invokestatic 辅助类
    code.extend_from_slice(&bridge_methodref.to_be_bytes());
    code.push(0xB0); // areturn
    code.push(0x57); // pop（丢掉 Throwable）
    code.push(0x2A); // aload_0
    code.push(0x2B); // aload_1
    code.push(0xB8); // invokestatic 原实现
    code.extend_from_slice(&renamed_methodref.to_be_bytes());
    code.push(0xB0); // areturn

    let mut exception_table = Vec::with_capacity(8);
    exception_table.extend_from_slice(&0u16.to_be_bytes()); // start_pc
    exception_table.extend_from_slice(&WRAPPER_HANDLER_OFFSET.to_be_bytes()); // end_pc
    exception_table.extend_from_slice(&WRAPPER_HANDLER_OFFSET.to_be_bytes()); // handler_pc
    exception_table.extend_from_slice(&throwable_class.to_be_bytes()); // catch_type

    // 只有一个帧：处理器入口。首个帧的 `offset_delta` 就是它的绝对偏移。
    let mut stack_map = Vec::with_capacity(18);
    stack_map.extend_from_slice(&1u16.to_be_bytes()); // number_of_entries
    stack_map.push(255); // full_frame
    stack_map.extend_from_slice(&WRAPPER_HANDLER_OFFSET.to_be_bytes());
    stack_map.extend_from_slice(&2u16.to_be_bytes()); // number_of_locals
    stack_map.push(ITEM_OBJECT);
    stack_map.extend_from_slice(&string_class.to_be_bytes());
    stack_map.push(ITEM_OBJECT);
    stack_map.extend_from_slice(&loader_class.to_be_bytes());
    stack_map.extend_from_slice(&1u16.to_be_bytes()); // number_of_stack_items
    stack_map.push(ITEM_OBJECT);
    stack_map.extend_from_slice(&throwable_class.to_be_bytes());

    WrapperBody {
        code,
        exception_table,
        stack_map,
    }
}

/// 拼出包装方法的 `Code` 属性（含异常表与 `StackMapTable`）。
///
/// 必须带 `StackMapTable`：目标类版本 ≥ 50 时，**只要方法里有分支/处理器**就要求帧信息，
/// 缺了 verifier 会直接抛 `VerifyError`（那会让整个类定义失败）。
fn encode_wrapper_code_attribute(
    name_index: &[u8],
    max_locals: u16,
    stack_map_name_index: &[u8],
    body: &WrapperBody,
) -> Result<Vec<u8>, String> {
    let code_length = u32::try_from(body.code.len()).map_err(|_| "方法体过长".to_string())?;
    let exception_length =
        u32::try_from(body.exception_table.len()).map_err(|_| "异常表过长".to_string())?;
    let stack_map_length =
        u32::try_from(body.stack_map.len()).map_err(|_| "StackMapTable 过长".to_string())?;
    // 子属性只有 StackMapTable 一个：name(2) + length(4) + info
    let attribute_length = 2u32 /*max_stack*/
        .checked_add(2)
        .and_then(|value| value.checked_add(4))
        .and_then(|value| value.checked_add(code_length))
        .and_then(|value| value.checked_add(2))
        .and_then(|value| value.checked_add(exception_length))
        .and_then(|value| value.checked_add(2))
        .and_then(|value| value.checked_add(6))
        .and_then(|value| value.checked_add(stack_map_length))
        .ok_or("Code 属性长度溢出")?;

    let mut attribute = Vec::with_capacity(usize::try_from(attribute_length).unwrap_or(0) + 6);
    attribute.extend_from_slice(name_index);
    attribute.extend_from_slice(&attribute_length.to_be_bytes());
    attribute.extend_from_slice(&2u16.to_be_bytes()); // max_stack
    attribute.extend_from_slice(&max_locals.to_be_bytes());
    attribute.extend_from_slice(&code_length.to_be_bytes());
    attribute.extend_from_slice(&body.code);
    attribute.extend_from_slice(&1u16.to_be_bytes()); // 异常表条数
    attribute.extend_from_slice(&body.exception_table);
    attribute.extend_from_slice(&1u16.to_be_bytes()); // 子属性条数
    attribute.extend_from_slice(stack_map_name_index);
    attribute.extend_from_slice(&stack_map_length.to_be_bytes());
    attribute.extend_from_slice(&body.stack_map);
    Ok(attribute)
}

/// 目标方法签名：`PatchEngine.collectPatches(String, ClassLoader) -> List`。
const COLLECT_PATCHES_DESCRIPTOR: &str =
    "(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;";
/// 目标方法名。
const COLLECT_PATCHES_NAME: &str = "collectPatches";

/// 走一遍常量池，返回它结束的位置（head 之后、字段/方法表之前）。
fn pool_end_offset(bytes: &[u8]) -> Result<usize, String> {
    let mut reader = Reader::new(bytes);
    reader.take(HEADER_LENGTH)?;
    let count = {
        let mut probe = Reader::new(bytes);
        probe.take(8)?;
        probe.u2()?
    };
    if count == 0 {
        return Err("class 文件常量池计数为 0".to_string());
    }
    let mut index = 1usize;
    while index < usize::from(count) {
        let tag = reader.u1()?;
        match tag {
            CONSTANT_UTF8 => {
                let length = usize::from(reader.u2()?);
                reader.take(length)?;
            }
            CONSTANT_CLASS | CONSTANT_STRING | CONSTANT_METHOD_TYPE | CONSTANT_MODULE
            | CONSTANT_PACKAGE => {
                reader.take(2)?;
            }
            CONSTANT_INTEGER | CONSTANT_FLOAT | CONSTANT_FIELDREF | CONSTANT_METHODREF
            | CONSTANT_INTERFACE_METHODREF | CONSTANT_NAME_AND_TYPE | CONSTANT_DYNAMIC
            | CONSTANT_INVOKE_DYNAMIC => {
                reader.take(4)?;
            }
            CONSTANT_METHOD_HANDLE => {
                reader.take(3)?;
            }
            CONSTANT_LONG | CONSTANT_DOUBLE => {
                reader.take(8)?;
                index += 2;
                continue;
            }
            other => return Err(format!("不认识的常量池标签 {other}")),
        }
        index += 1;
    }
    Ok(reader.offset)
}

/// 把 `PatchEngine.collectPatches` 的方法体改成"调用辅助类并直接返回"。
///
/// 装配方式（只动两处，其余字节原样搬运）：
/// 1. 头部常量池计数改成新值，原池子后追加新条目；
/// 2. **整段重建目标方法的 `Code` 属性**：`max_stack=2`、`max_locals` 保持不变、
///    `code_length=6`、方法体换成 `encode_short_circuit_body()`、异常表与子属性清空。
///
/// 第 2 步必须重建而不是就地改：原方法带分支时 `Code` 里会有 `StackMapTable`，
/// 替换掉分支后那些帧的偏移全部失配，verifier 会直接抛 `VerifyError`；
/// 而"只改字节、不改长度"又会把异常表/子属性的位置搞乱。整段重建把这两件事一次解决。
fn rewrite_collect_patches(original: &[u8], helper_class: &str) -> Result<Vec<u8>, String> {
    let class = ClassFile::parse(original)?;
    let pool_count = u16::try_from(class.pool.len()).map_err(|_| "常量池过大".to_string())?;
    let methods = parse_methods(original, &class.pool)?;
    let target = methods
        .iter()
        .find(|method| {
            method.name == COLLECT_PATCHES_NAME && method.descriptor == COLLECT_PATCHES_DESCRIPTOR
        })
        .ok_or_else(|| {
            format!("找不到 {COLLECT_PATCHES_NAME}{COLLECT_PATCHES_DESCRIPTOR}（版本不兼容）")
        })?;
    let code = target.code.ok_or("目标方法没有 Code 属性")?;
    if code.offset < 14 {
        return Err("Code 属性位置异常".to_string());
    }

    let (addition, new_count, methodref) = encode_static_call_reference(
        pool_count,
        helper_class,
        "collect",
        COLLECT_PATCHES_DESCRIPTOR,
    )?;
    let body = encode_short_circuit_body(methodref);

    // Code 属性内部布局：name_index(-14) / attribute_length(-12) / max_stack(-8)
    //   / max_locals(-6) / code_length(-4) / 字节码(0…)
    let attribute_name = original
        .get(code.offset - 14..code.offset - 12)
        .ok_or("读不到 Code 属性名")?;
    let attribute_length = u32::from_be_bytes(
        original
            .get(code.offset - 12..code.offset - 8)
            .ok_or("读不到 Code 属性长度")?
            .try_into()
            .map_err(|_| "Code 属性长度读取失败".to_string())?,
    );
    let max_locals = original
        .get(code.offset - 6..code.offset - 4)
        .ok_or("读不到 max_locals")?;
    let attribute_start = code.offset - 14;
    let attribute_end = usize::try_from(attribute_length)
        .map_err(|_| "Code 属性长度过大".to_string())?
        .checked_add(code.offset - 8)
        .ok_or("Code 属性越界")?;
    if attribute_end > original.len() {
        return Err("Code 属性超出文件末尾".to_string());
    }

    // 新的 Code 属性：长度 = 2(max_stack) + 2(max_locals) + 4(code_length) + 6(body)
    //   + 2(异常表计数) + 2(子属性计数) = 18
    let mut attribute = Vec::with_capacity(20);
    attribute.extend_from_slice(attribute_name);
    attribute.extend_from_slice(&18u32.to_be_bytes());
    attribute.extend_from_slice(&2u16.to_be_bytes());
    attribute.extend_from_slice(max_locals);
    attribute.extend_from_slice(&(body.len() as u32).to_be_bytes());
    attribute.extend_from_slice(&body);
    attribute.extend_from_slice(&0u16.to_be_bytes());
    attribute.extend_from_slice(&0u16.to_be_bytes());

    let pool_end = pool_end_offset(original)?;
    let mut out = Vec::with_capacity(original.len() + addition.bytes.len());
    out.extend_from_slice(&original[..8]);
    out.extend_from_slice(&new_count.to_be_bytes());
    out.extend_from_slice(&original[HEADER_LENGTH..pool_end]);
    out.extend_from_slice(&addition.bytes);
    out.extend_from_slice(&original[pool_end..attribute_start]);
    out.extend_from_slice(&attribute);
    out.extend_from_slice(&original[attribute_end..]);
    Ok(out)
}

/// 一个方法在文件里的精确位置：改名要的就是 `name_index_offset` 这两字节。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MethodLayout {
    name: String,
    descriptor: String,
    descriptor_index: u16,
    /// `method_info` 里 `name_index` 字段的绝对偏移。
    name_index_offset: usize,
    /// `Code` 属性整体区间（含 6 字节属性头），没有则 None。
    code_attribute: Option<(usize, usize)>,
    max_locals: u16,
}

/// class 文件的关键布局：常量池与方法表的边界，以及每个方法的位置。
#[derive(Debug, Clone)]
struct Layout {
    pool_count: u16,
    pool_end: usize,
    this_class: u16,
    method_table_start: usize,
    method_table_end: usize,
    methods: Vec<MethodLayout>,
}

/// 走一遍文件拿到布局：改写"包装"要靠它精确定位（改哪个字节、往哪里插新方法）。
fn layout(bytes: &[u8], pool: &[Option<PoolEntry>]) -> Result<Layout, String> {
    let utf8 = |index: u16| -> String {
        match pool.get(usize::from(index)) {
            Some(Some(PoolEntry::Utf8(text))) => text.clone(),
            _ => String::new(),
        }
    };

    let pool_end = pool_end_offset(bytes)?;
    let pool_count = {
        let mut probe = Reader::new(bytes);
        probe.take(8)?;
        probe.u2()?
    };

    let mut reader = Reader::new(bytes);
    reader.seek(pool_end)?;
    let _access_flags = reader.u2()?;
    let this_class = reader.u2()?;
    let _super_class = reader.u2()?;
    let interfaces = reader.u2()?;
    reader.take(usize::from(interfaces) * 2)?;
    let fields = reader.u2()?;
    for _ in 0..fields {
        reader.take(6)?;
        skip_attributes(&mut reader)?;
    }

    let method_table_start = reader.offset;
    let method_count = reader.u2()?;
    let mut methods = Vec::with_capacity(usize::from(method_count));
    for _ in 0..method_count {
        let _access = reader.u2()?;
        let name_index_offset = reader.offset;
        let name_index = reader.u2()?;
        let descriptor_index = reader.u2()?;
        let attribute_count = reader.u2()?;
        let mut code_attribute = None;
        let mut max_locals = 0u16;
        for _ in 0..attribute_count {
            let attribute_start = reader.offset;
            let attribute_name = utf8(reader.u2()?);
            let length = usize::try_from(reader.u4()?).map_err(|_| "属性长度过大".to_string())?;
            let attribute_end = reader
                .offset
                .checked_add(length)
                .ok_or_else(|| "属性越界".to_string())?;
            if attribute_name == "Code" && attribute_end <= bytes.len() {
                // Code 属性 info 布局：max_stack(+6) / max_locals(+8) / code_length(+10) / 字节码(+14)
                let locals = bytes
                    .get(attribute_start + 8..attribute_start + 10)
                    .ok_or_else(|| "读不到 max_locals".to_string())?;
                max_locals = u16::from_be_bytes([locals[0], locals[1]]);
                code_attribute = Some((attribute_start, attribute_end));
            }
            reader.seek(attribute_end)?;
        }
        methods.push(MethodLayout {
            name: utf8(name_index),
            descriptor: utf8(descriptor_index),
            descriptor_index,
            name_index_offset,
            code_attribute,
            max_locals,
        });
    }
    let method_table_end = reader.offset;

    Ok(Layout {
        pool_count,
        pool_end,
        this_class,
        method_table_start,
        method_table_end,
        methods,
    })
}

/// 原实现被改名后用的名字，必须与辅助类里的 `ORIGINAL_METHOD` 一致。
const RENAMED_COLLECT_PATCHES: &str = "collectPatches$pzm";
/// 辅助类里被调用的方法名。
const BRIDGE_METHOD: &str = "collect";

/// 拼一个 `Code` 属性（只含最基本内容：无异常表、无子属性）。
fn encode_code_attribute(
    name_index: &[u8],
    max_locals: u16,
    body: &[u8],
) -> Result<Vec<u8>, String> {
    let body_length = u32::try_from(body.len()).map_err(|_| "方法体过长".to_string())?;
    // 2(max_stack) + 2(max_locals) + 4(code_length) + body + 2(异常表) + 2(子属性)
    let length = 12u32
        .checked_add(body_length)
        .ok_or("Code 属性长度溢出")?;
    let mut attribute = Vec::with_capacity(usize::try_from(length).unwrap_or(0) + 6);
    attribute.extend_from_slice(name_index);
    attribute.extend_from_slice(&length.to_be_bytes());
    attribute.extend_from_slice(&2u16.to_be_bytes()); // max_stack
    attribute.extend_from_slice(&max_locals.to_be_bytes());
    attribute.extend_from_slice(&body_length.to_be_bytes());
    attribute.extend_from_slice(body);
    attribute.extend_from_slice(&0u16.to_be_bytes()); // 异常表
    attribute.extend_from_slice(&0u16.to_be_bytes()); // 子属性
    Ok(attribute)
}

/// 这个类文件里是否已经声明了同名方法。
///
/// 用途是**幂等**：`collectPatches` 一旦被包装过，再包一次就会让 `collectPatches$pzm`
/// 撞名（第二次改名会产生两个同名同签名的方法）→ `ClassFormatError`。
/// 任何解析失败都返回 `false`（宁可不动，交给调用方放行原字节）。
pub(crate) fn declares_method(bytes: &[u8], name: &str) -> bool {
    let Ok(class) = ClassFile::parse(bytes) else {
        return false;
    };
    match parse_methods(bytes, &class.pool) {
        Ok(methods) => methods.iter().any(|method| method.name == name),
        Err(_) => false,
    }
}

/// 包装式改写：让 `collectPatches` 先问辅助类，失败再由**原实现**兜底。
///
/// 1. 原方法改名为 `collectPatches$pzm`（新增独立 Utf8 后只改它自己的 `name_index`，
///    所以 ZombieBuddy 调用点引用的 `collectPatches` 名字不受影响）；
/// 2. 追加一个新的同名方法 `collectPatches(String, ClassLoader)`，体为
///    `try { FastLaunchBridge.collect(a,b) } catch (Throwable) { collectPatches$pzm(a,b) }`
///    —— **注意必须有 catch**：打开类加载 hook 的时机（`Agent_OnLoad`）比辅助类能进 JVM 的
///    时机（`VMInit`）更早，中间的调用靠这层兜底走原实现，见 `encode_wrapper_body`；
/// 3. 原方法体**一个字节都不动**——辅助类回退时调的就是它。
///
/// 与"直接替换方法体"相比，这样才真正满足"任何失败回退原生扫描"：回退的是
/// ZombieBuddy 自己的实现，而不是我们复刻的版本。
pub(crate) fn wrap_collect_patches(original: &[u8], bridge_class: &str) -> Result<Vec<u8>, String> {
    let class = ClassFile::parse(original)?;
    let info = layout(original, &class.pool)?;
    let target = info
        .methods
        .iter()
        .find(|method| {
            method.name == COLLECT_PATCHES_NAME && method.descriptor == COLLECT_PATCHES_DESCRIPTOR
        })
        .ok_or_else(|| {
            format!("找不到 {COLLECT_PATCHES_NAME}{COLLECT_PATCHES_DESCRIPTOR}（版本不兼容）")
        })?;
    let (code_start, _code_end) = target.code_attribute.ok_or("目标方法没有 Code 属性")?;
    if target.name_index_offset < 2 {
        return Err("方法 name_index 位置异常".to_string());
    }
    let original_name_index = u16::from_be_bytes(
        original
            .get(target.name_index_offset..target.name_index_offset + 2)
            .ok_or("读不到方法原名索引")?
            .try_into()
            .map_err(|_| "方法原名索引读取失败".to_string())?,
    );
    let access_flags = original
        .get(target.name_index_offset - 2..target.name_index_offset)
        .ok_or("读不到方法访问标志")?;
    let code_name_index = original
        .get(code_start..code_start + 2)
        .ok_or("读不到 Code 属性名索引")?
        .to_vec();

    // 追加的常量池条目（重复的 Utf8 合法，换取"不猜已有索引"的简单性）。
    let mut next = info.pool_count;
    let mut addition: Vec<u8> = Vec::new();
    let mut push_utf8 = |addition: &mut Vec<u8>, text: &str, next: &mut u16| -> Result<u16, String> {
        let index = *next;
        addition.extend_from_slice(&encode_utf8(text)?);
        *next = next.checked_add(1).ok_or("常量池索引溢出")?;
        Ok(index)
    };
    let renamed_index = push_utf8(&mut addition, RENAMED_COLLECT_PATCHES, &mut next)?;
    let bridge_name_index = push_utf8(&mut addition, bridge_class, &mut next)?;
    let bridge_class_index = next;
    addition.extend_from_slice(&encode_class(bridge_name_index));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;
    let collect_name_index = push_utf8(&mut addition, BRIDGE_METHOD, &mut next)?;
    let code_name_index = push_utf8(&mut addition, "Code", &mut next)?;

    // 改名后的原实现：NameAndType + Methodref（复用原描述符 Utf8）
    let renamed_nat = next;
    addition.extend_from_slice(&encode_name_and_type(
        renamed_index,
        target.descriptor_index,
    ));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;
    let renamed_methodref = next;
    addition.extend_from_slice(&encode_methodref(info.this_class, renamed_nat));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    // 辅助类：NameAndType + Methodref（同样复用原描述符 Utf8）
    let bridge_nat = next;
    addition.extend_from_slice(&encode_name_and_type(
        collect_name_index,
        target.descriptor_index,
    ));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;
    let bridge_methodref = next;
    addition.extend_from_slice(&encode_methodref(bridge_class_index, bridge_nat));
    next = next.checked_add(1).ok_or("常量池索引溢出")?;

    // 包装体的 `catch (Throwable)` 与 StackMapTable 需要这三个 Class 常量。
    // 重复的 Utf8/Class 条目合法，所以不费劲去复用已有索引。
    let mut push_class = |addition: &mut Vec<u8>, name: &str, next: &mut u16| -> Result<u16, String> {
        let name_index = push_utf8(addition, name, next)?;
        let class_index = *next;
        addition.extend_from_slice(&encode_class(name_index));
        *next = next.checked_add(1).ok_or("常量池索引溢出")?;
        Ok(class_index)
    };
    let throwable_class_index = push_class(&mut addition, "java/lang/Throwable", &mut next)?;
    let string_class_index = push_class(&mut addition, "java/lang/String", &mut next)?;
    let loader_class_index = push_class(&mut addition, "java/lang/ClassLoader", &mut next)?;
    let stack_map_name_index = push_utf8(&mut addition, "StackMapTable", &mut next)?;
    let new_pool_count = next;

    // 追加的同名包装方法：签名与原方法一致，体是"调辅助类，失败回退原实现"。
    let body = encode_wrapper_body(
        bridge_methodref,
        renamed_methodref,
        string_class_index,
        loader_class_index,
        throwable_class_index,
    );
    let code_attribute = encode_wrapper_code_attribute(
        &code_name_index.to_be_bytes(),
        target.max_locals,
        &stack_map_name_index.to_be_bytes(),
        &body,
    )?;
    let mut new_method = Vec::new();
    new_method.extend_from_slice(access_flags);
    new_method.extend_from_slice(&original_name_index.to_be_bytes());
    new_method.extend_from_slice(&target.descriptor_index.to_be_bytes());
    new_method.extend_from_slice(&1u16.to_be_bytes()); // 一个属性
    new_method.extend_from_slice(&code_attribute);

    // 装配：头（新计数）+ 原池子 + 增量 + 到改名处 + 改名 + 到方法表末尾
    //       + 新方法 + 类属性。
    let mut out = Vec::with_capacity(original.len() + addition.len() + new_method.len());
    out.extend_from_slice(&original[..8]);
    out.extend_from_slice(&new_pool_count.to_be_bytes());
    out.extend_from_slice(&original[HEADER_LENGTH..info.pool_end]);
    out.extend_from_slice(&addition);
    out.extend_from_slice(&original[info.pool_end..info.method_table_start]);
    let method_count = u16::from_be_bytes(
        original[info.method_table_start..info.method_table_start + 2]
            .try_into()
            .map_err(|_| "读不到方法计数".to_string())?,
    );
    out.extend_from_slice(
        &method_count
            .checked_add(1)
            .ok_or("方法数溢出")?
            .to_be_bytes(),
    );
    out.extend_from_slice(&original[info.method_table_start + 2..target.name_index_offset]);
    out.extend_from_slice(&renamed_index.to_be_bytes());
    out.extend_from_slice(&original[target.name_index_offset + 2..info.method_table_end]);
    out.extend_from_slice(&new_method);
    out.extend_from_slice(&original[info.method_table_end..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工拼一个最小 class 文件：幻数 + 版本 + 常量池（Utf8/Class/String/NameAndType/long）。
    fn sample_class() -> Vec<u8> {
        sample_class_without_methods()
    }

    fn sample_class_without_methods() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes()); // minor
        bytes.extend_from_slice(&52u16.to_be_bytes()); // major = Java 8
        bytes.extend_from_slice(&8u16.to_be_bytes()); // 常量池计数（索引 1..=7）

        // #1 Utf8 "collectPatches"
        bytes.push(CONSTANT_UTF8);
        let name = b"collectPatches";
        bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(name);

        // #2 Utf8 "(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;"
        bytes.push(CONSTANT_UTF8);
        let descriptor = b"(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;";
        bytes.extend_from_slice(&(descriptor.len() as u16).to_be_bytes());
        bytes.extend_from_slice(descriptor);

        // #3 Utf8 "me/zed_0xff/zombie_buddy/PatchEngine"
        bytes.push(CONSTANT_UTF8);
        let class_name = b"me/zed_0xff/zombie_buddy/PatchEngine";
        bytes.extend_from_slice(&(class_name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(class_name);

        // #4 Class -> #3
        bytes.push(CONSTANT_CLASS);
        bytes.extend_from_slice(&3u16.to_be_bytes());

        // #5 String -> #2
        bytes.push(CONSTANT_STRING);
        bytes.extend_from_slice(&2u16.to_be_bytes());

        // #6 NameAndType -> #1:#2
        bytes.push(CONSTANT_NAME_AND_TYPE);
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());

        // #7 long（占 #7/#8 两个槽位，所以计数写成 9）
        bytes[8..10].copy_from_slice(&9u16.to_be_bytes());
        bytes.push(CONSTANT_LONG);
        bytes.extend_from_slice(&123456789i64.to_be_bytes());

        bytes
    }

    #[test]
    fn parses_constant_pool() {
        let class = ClassFile::parse(&sample_class()).expect("应当解析成功");
        assert_eq!(class.major, 52);
        assert_eq!(class.utf8(1), Some("collectPatches"));
        assert_eq!(
            class.utf8(2),
            Some("(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;")
        );
        assert_eq!(class.utf8(3), Some("me/zed_0xff/zombie_buddy/PatchEngine"));
        assert_eq!(class.pool.len(), 9);
        assert!(matches!(class.pool[7], Some(PoolEntry::Other { tag: CONSTANT_LONG, .. })));
        // long 的第二槽位必须是空的。
        assert!(class.pool[8].is_none());
        assert_eq!(class.utf8(99), None);
    }

    #[test]
    fn locates_collect_patches_code_attribute() {
        // 在基础常量池之上补一个 "Code" Utf8（#9），再加最小的类/方法结构。
        let mut bytes = sample_class_without_methods();
        bytes[8..10].copy_from_slice(&10u16.to_be_bytes()); // 常量池计数 9 -> 10
        bytes.push(CONSTANT_UTF8);
        let code_name = b"Code";
        bytes.extend_from_slice(&(code_name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(code_name);

        bytes.extend_from_slice(&0x0021u16.to_be_bytes()); // access_flags
        bytes.extend_from_slice(&4u16.to_be_bytes()); // this_class -> #4
        bytes.extend_from_slice(&4u16.to_be_bytes()); // super_class -> #4
        bytes.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        bytes.extend_from_slice(&0u16.to_be_bytes()); // fields

        bytes.extend_from_slice(&1u16.to_be_bytes()); // methods
        bytes.extend_from_slice(&0x0009u16.to_be_bytes()); // public static
        bytes.extend_from_slice(&1u16.to_be_bytes()); // name -> #1 collectPatches
        bytes.extend_from_slice(&2u16.to_be_bytes()); // descriptor -> #2
        bytes.extend_from_slice(&1u16.to_be_bytes()); // attributes
        bytes.extend_from_slice(&9u16.to_be_bytes()); // attribute name -> #9 Code
        bytes.extend_from_slice(&12u32.to_be_bytes()); // attribute length
        bytes.extend_from_slice(&1u16.to_be_bytes()); // max_stack
        bytes.extend_from_slice(&2u16.to_be_bytes()); // max_locals
        bytes.extend_from_slice(&4u32.to_be_bytes()); // code_length
        let code = [0xB1u8, 0x00, 0x00, 0x00]; // 故意用"看起来像指令"的字节占位
        bytes.extend_from_slice(&code);
        bytes.extend_from_slice(&0u16.to_be_bytes()); // class attributes

        let class = ClassFile::parse(&bytes).expect("常量池应当解析成功");
        let methods = parse_methods(&bytes, &class.pool).expect("方法表应当解析成功");
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name, "collectPatches");
        assert_eq!(
            methods[0].descriptor,
            "(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;"
        );
        let code_range = methods[0].code.expect("应当定位到 Code 属性");
        assert_eq!(code_range.length, 4);
        assert_eq!(&bytes[code_range.offset..code_range.offset + 4], &code);
    }

    #[test]
    fn encodes_static_call_pool_entries() {
        let (addition, new_count, methodref) = encode_static_call_reference(
            9,
            "me/zed_0xff/zombie_buddy/FastLaunchBridge",
            "collect",
            "(Ljava/lang/String;Ljava/lang/ClassLoader;Ljava/lang/String;)Ljava/util/List;",
        )
        .expect("编码应当成功");

        // 6 个新条目：Utf8(类名) / Class / Utf8(方法名) / Utf8(描述符) / NameAndType / Methodref
        assert_eq!(addition.slots, 6);
        assert_eq!(new_count, 15);
        assert_eq!(methodref, 14);

        // 增量字节可以当作常量池尾部继续解析：把计数改成新值后重新解析必须成功。
        let mut rebuilt = sample_class_without_methods();
        rebuilt[8..10].copy_from_slice(&new_count.to_be_bytes());
        rebuilt.extend_from_slice(&addition.bytes);
        let class = ClassFile::parse(&rebuilt).expect("追加后的常量池应当仍然可解析");
        assert_eq!(class.pool.len(), usize::from(new_count));
        assert_eq!(class.utf8(9), Some("me/zed_0xff/zombie_buddy/FastLaunchBridge"));
        assert_eq!(class.utf8(11), Some("collect"));
        assert_eq!(
            class.utf8(12),
            Some("(Ljava/lang/String;Ljava/lang/ClassLoader;Ljava/lang/String;)Ljava/util/List;")
        );
        assert!(matches!(class.pool[10], Some(PoolEntry::Class(9))));
        assert!(matches!(
            class.pool[13],
            Some(PoolEntry::NameAndType {
                name: 11,
                descriptor: 12
            })
        ));
        assert!(matches!(class.pool[14], Some(PoolEntry::Other { tag: CONSTANT_METHODREF, .. })));
    }

    #[test]
    fn encodes_short_circuit_body() {
        let body = encode_short_circuit_body(14);
        assert_eq!(body, vec![0x2A, 0x2B, 0xB8, 0x00, 0x0E, 0xB0]);
        // invokestatic 的操作数就是传进来的 Methodref 索引（大端）。
        assert_eq!(u16::from_be_bytes([body[3], body[4]]), 14);
        // 只碰局部变量 0/1，不新增局部变量；栈深需求为 2。
        assert_eq!(body[0], 0x2A);
        assert_eq!(body[1], 0x2B);
        assert_eq!(*body.last().unwrap(), 0xB0);
    }

    /// 带一个 `public static collectPatches(String, ClassLoader)` 方法与 `Code` 属性的 fixture。
    fn sample_class_with_method() -> Vec<u8> {
        let mut bytes = sample_class_without_methods();
        bytes[8..10].copy_from_slice(&10u16.to_be_bytes()); // 常量池计数 9 -> 10
        bytes.push(CONSTANT_UTF8);
        let code_name = b"Code";
        bytes.extend_from_slice(&(code_name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(code_name);

        bytes.extend_from_slice(&0x0021u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(&4u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());

        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&0x0009u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&9u16.to_be_bytes());
        bytes.extend_from_slice(&12u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.extend_from_slice(&4u32.to_be_bytes());
        bytes.extend_from_slice(&[0xB1u8, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes
    }

    #[test]
    fn rewrites_collect_patches_into_short_circuit() {
        let original = sample_class_with_method();
        let rewritten = rewrite_collect_patches(&original, "me/zed_0xff/zombie_buddy/FastLaunchBridge")
            .expect("改写应当成功");

        // 改写后的文件必须还能被自己的解析器读懂，且目标方法体就是短路字节码。
        let class = ClassFile::parse(&rewritten).expect("改写结果应当仍是合法 class");
        let methods = parse_methods(&rewritten, &class.pool).expect("方法表应当可解析");
        let target = methods
            .iter()
            .find(|method| method.name == COLLECT_PATCHES_NAME)
            .expect("应当仍然有 collectPatches");
        let code = target.code.expect("应当有 Code 属性");
        // 原池计数是 10（#9 是 "Code"），所以新条目从 #10 开始，Methodref 落在 #15。
        let expected = encode_short_circuit_body(15);
        assert_eq!(code.length, expected.len());
        assert_eq!(&rewritten[code.offset..code.offset + code.length], &expected[..]);
        // max_stack 提到 2，max_locals 保持原样。
        assert_eq!(
            u16::from_be_bytes([rewritten[code.offset - 8], rewritten[code.offset - 7]]),
            2
        );
        assert_eq!(
            u16::from_be_bytes([rewritten[code.offset - 6], rewritten[code.offset - 5]]),
            2
        );
        // 常量池长出 6 个条目（10 -> 16），头里的计数也要跟着变。
        assert_eq!(class.pool.len(), 16);
        assert_eq!(class.utf8(10), Some("me/zed_0xff/zombie_buddy/FastLaunchBridge"));
        assert_eq!(class.utf8(12), Some("collect"));
        assert_eq!(
            u16::from_be_bytes([rewritten[8], rewritten[9]]),
            16
        );
    }

    /// 用**真实**的 `PatchEngine.class` 跑一遍改写，验证真实类的布局与实现假定一致。
    ///
    /// 路径由环境变量 `PZM_PATCH_ENGINE_CLASS` 指定（从游戏目录 `ZombieBuddy.jar` 里取出来）；
    /// 没设就跳过，所以测试不依赖游戏安装。
    #[test]
    fn rewrites_real_patch_engine_class_when_provided() {
        let Ok(path) = std::env::var("PZM_PATCH_ENGINE_CLASS") else {
            return;
        };
        let original = std::fs::read(&path).expect("读不到 PatchEngine.class");
        let class = ClassFile::parse(&original).expect("真实常量池应当能解析");
        let pool_count = u16::from_be_bytes([original[8], original[9]]);
        assert_eq!(usize::from(pool_count), class.pool.len());

        let methods = parse_methods(&original, &class.pool).expect("真实方法表应当能解析");
        let original_method_count = methods.len();
        assert!(
            methods
                .iter()
                .any(|method| method.name == COLLECT_PATCHES_NAME),
            "真实类里应当有 collectPatches"
        );
        assert!(
            methods.iter().any(|method| {
                method.name == COLLECT_PATCHES_NAME
                    && method.descriptor == COLLECT_PATCHES_DESCRIPTOR
            }),
            "真实 collectPatches 的签名应当与假定一致"
        );

        let rewritten =
            rewrite_collect_patches(&original, "me/zed_0xff/zombie_buddy/FastLaunchBridge")
                .expect("真实 PatchEngine 应当能改写");
        let rewritten_class = ClassFile::parse(&rewritten).expect("改写结果应当仍是合法 class");
        let rewritten_methods =
            parse_methods(&rewritten, &rewritten_class.pool).expect("改写后方法表应当可解析");
        assert_eq!(rewritten_methods.len(), original_method_count, "方法数量不应变化");

        let target = rewritten_methods
            .iter()
            .find(|method| method.name == COLLECT_PATCHES_NAME)
            .expect("改写后仍然要有 collectPatches");
        let code = target.code.expect("改写后应当有 Code 属性");
        // 新条目从 pool_count 开始，Methodref 是第 6 个新条目。
        let expected = encode_short_circuit_body(pool_count + 5);
        assert_eq!(code.length, expected.len());
        assert_eq!(&rewritten[code.offset..code.offset + code.length], &expected[..]);
        assert_eq!(
            u16::from_be_bytes([rewritten[code.offset - 8], rewritten[code.offset - 7]]),
            2
        );
        assert_eq!(
            u16::from_be_bytes([rewritten[8], rewritten[9]]),
            pool_count + 6
        );
    }

    /// 用 javac 的真实产物交叉验证解析器：它是真正编译出来的 class（含 StackMapTable、
    /// 字符串常量、异常表），比手拼 fixture 更能说明解析器经得起真实输入。
    #[test]
    fn parses_the_compiled_bridge_class() {
        let bytes = crate::fastlaunch::FAST_LAUNCH_BRIDGE_CLASS;
        let class = ClassFile::parse(bytes).expect("javac 产物应当能被解析");
        assert!(class.major >= 52, "应当至少有 Java 8 的版本号");
        let methods = parse_methods(bytes, &class.pool).expect("方法表应当能解析");
        assert!(
            methods.iter().any(|method| {
                method.name == "collect"
                    && method.descriptor
                        == "(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;"
            }),
            "辅助类必须提供与 collectPatches 同签名的 collect"
        );
        assert!(
            methods.iter().any(|method| method.name == "callOriginal"),
            "辅助类必须保留调用原实现的入口"
        );
        assert!(
            methods.iter().filter(|method| method.code.is_some()).count() >= 3,
            "应当有多个带 Code 的方法"
        );
    }

    /// 解析 `CONSTANT_Class` 条目指向的类名（测试用；`ClassFile::utf8` 只接 Utf8 索引）。
    fn class_entry_name(class: &ClassFile, index: u16) -> Option<String> {
        match class.pool.get(usize::from(index))? {
            Some(PoolEntry::Class(name_index)) => class.utf8(*name_index).map(str::to_string),
            _ => None,
        }
    }

    #[test]
    fn wraps_collect_patches_and_keeps_original_body() {
        let original = sample_class_with_method();
        let wrapped = wrap_collect_patches(&original, "pzm/fastlaunch/FastLaunchBridge")
            .expect("包装应当成功");
        let class = ClassFile::parse(&wrapped).expect("包装结果应当仍是合法 class");
        let methods = parse_methods(&wrapped, &class.pool).expect("方法表应当能解析");

        // 方法数 +1：原实现改名保留，另有一个同名包装方法。
        assert_eq!(methods.len(), 2);
        let renamed = methods
            .iter()
            .find(|method| method.name == RENAMED_COLLECT_PATCHES)
            .expect("原实现应当被改名保留");
        let renamed_code = renamed.code.expect("改名后的方法仍应有 Code");
        assert_eq!(
            &wrapped[renamed_code.offset..renamed_code.offset + 4],
            &[0xB1u8, 0x00, 0x00, 0x00],
            "原实现的字节码不能被改动"
        );

        let wrapper = methods
            .iter()
            .find(|method| method.name == COLLECT_PATCHES_NAME)
            .expect("应当有同名包装方法");
        assert_eq!(wrapper.descriptor, COLLECT_PATCHES_DESCRIPTOR, "签名必须与原方法一致");
        let wrapper_code = wrapper.code.expect("包装方法应当有 Code");
        // 包装体：try { 调辅助类 } catch (Throwable) { 调原实现 }，共 13 字节。
        assert_eq!(wrapper_code.length, 13, "包装体长度应为 13（含 catch 兜底）");
        let body = &wrapped[wrapper_code.offset..wrapper_code.offset + wrapper_code.length];
        assert_eq!(body[0], 0x2A, "aload_0");
        assert_eq!(body[1], 0x2B, "aload_1");
        assert_eq!(body[2], 0xB8, "invokestatic 辅助类");
        assert_eq!(body[5], 0xB0, "areturn");
        assert_eq!(body[6], 0x57, "pop（异常处理器入口）");
        assert_eq!(body[7], 0x2A, "aload_0");
        assert_eq!(body[8], 0x2B, "aload_1");
        assert_eq!(body[9], 0xB8, "invokestatic 原实现");
        assert_eq!(body[12], 0xB0, "areturn");
        // 两条 invokestatic 指向不同方法（辅助类 vs 本类的 collectPatches$pzm）。
        assert_ne!(&body[3..5], &body[10..12], "兜底必须调被改名的原实现");

        // 异常表与子属性紧跟在字节码之后（`code` 只覆盖字节码，所以要按属性起点读）。
        let tail = &wrapped[wrapper_code.offset + wrapper_code.length..];
        // 异常表：`[0, 6) -> 6`，catch_type = java/lang/Throwable。
        assert_eq!(u16::from_be_bytes([tail[0], tail[1]]), 1, "必须有一条 catch-all 记录");
        let entry = &tail[2..10];
        assert_eq!(u16::from_be_bytes([entry[0], entry[1]]), 0, "start_pc");
        assert_eq!(u16::from_be_bytes([entry[2], entry[3]]), 6, "end_pc");
        assert_eq!(u16::from_be_bytes([entry[4], entry[5]]), 6, "handler_pc");
        let catch_type = u16::from_be_bytes([entry[6], entry[7]]);
        assert_eq!(
            class_entry_name(&class, catch_type).as_deref(),
            Some("java/lang/Throwable"),
            "catch_type 必须是 java/lang/Throwable"
        );

        // 子属性：必须带 StackMapTable（有分支/处理器时缺了会 VerifyError）。
        assert_eq!(u16::from_be_bytes([tail[10], tail[11]]), 1, "应当只有一个子属性");
        let attribute_name_index = u16::from_be_bytes([tail[12], tail[13]]);
        assert_eq!(class.utf8(attribute_name_index), Some("StackMapTable"));
        let stack_map_length = u32::from_be_bytes([tail[14], tail[15], tail[16], tail[17]]);
        assert_eq!(stack_map_length, 18, "一个 full_frame 的长度");
        // info：number_of_entries(2) + full_frame(16)
        assert_eq!(u16::from_be_bytes([tail[18], tail[19]]), 1, "只有一个帧");
        assert_eq!(tail[20], 255, "full_frame");
        assert_eq!(
            u16::from_be_bytes([tail[21], tail[22]]),
            6,
            "首个帧的 offset_delta = 处理器偏移"
        );
        // full_frame: locals = [String, ClassLoader]、stack = [Throwable]
        assert_eq!(u16::from_be_bytes([tail[23], tail[24]]), 2, "两个局部变量");
        assert_eq!(tail[25], ITEM_OBJECT);
        let string_class = u16::from_be_bytes([tail[26], tail[27]]);
        assert_eq!(class_entry_name(&class, string_class).as_deref(), Some("java/lang/String"));
        assert_eq!(tail[28], ITEM_OBJECT);
        let loader_class = u16::from_be_bytes([tail[29], tail[30]]);
        assert_eq!(
            class_entry_name(&class, loader_class).as_deref(),
            Some("java/lang/ClassLoader")
        );
        assert_eq!(u16::from_be_bytes([tail[31], tail[32]]), 1, "栈上有一个 Throwable");
        assert_eq!(tail[33], ITEM_OBJECT);
        let throwable_class = u16::from_be_bytes([tail[34], tail[35]]);
        assert_eq!(
            class_entry_name(&class, throwable_class).as_deref(),
            Some("java/lang/Throwable")
        );

        // 幂等判断：包装前没有、包装后一定有 `collectPatches$pzm`。
        assert!(!declares_method(&original, RENAMED_COLLECT_PATCHES));
        assert!(declares_method(&wrapped, RENAMED_COLLECT_PATCHES));

        // 头部计数与解析结果必须自洽。
        assert_eq!(
            usize::from(u16::from_be_bytes([wrapped[8], wrapped[9]])),
            class.pool.len()
        );
    }

    #[test]
    fn wraps_real_patch_engine_class_when_provided() {
        let Ok(path) = std::env::var("PZM_PATCH_ENGINE_CLASS") else {
            return;
        };
        let original = std::fs::read(&path).expect("读不到 PatchEngine.class");
        let original_class = ClassFile::parse(&original).expect("真实常量池应当能解析");
        let original_methods =
            parse_methods(&original, &original_class.pool).expect("真实方法表应当能解析");
        let original_target = original_methods
            .iter()
            .find(|method| method.name == COLLECT_PATCHES_NAME)
            .expect("真实类里应当有 collectPatches");
        let original_code = original_target.code.expect("真实方法应当有 Code");

        let wrapped = wrap_collect_patches(&original, "pzm/fastlaunch/FastLaunchBridge")
            .expect("真实 PatchEngine 应当能包装");
        let class = ClassFile::parse(&wrapped).expect("包装结果应当仍是合法 class");
        let methods = parse_methods(&wrapped, &class.pool).expect("包装后方法表应当能解析");

        assert_eq!(methods.len(), original_methods.len() + 1, "只应多出一个包装方法");
        let renamed = methods
            .iter()
            .find(|method| method.name == RENAMED_COLLECT_PATCHES)
            .expect("真实类里原实现应当被改名保留");
        let renamed_code = renamed.code.expect("改名后的方法仍应有 Code");
        // 最强的不变量：原实现的字节码逐字节不变。
        assert_eq!(
            &wrapped[renamed_code.offset..renamed_code.offset + renamed_code.length],
            &original[original_code.offset..original_code.offset + original_code.length]
        );
        assert!(
            methods.iter().any(|method| {
                method.name == COLLECT_PATCHES_NAME
                    && method.descriptor == COLLECT_PATCHES_DESCRIPTOR
            }),
            "包装方法的签名必须与原方法一致"
        );
    }

    #[test]
    fn rejects_broken_input() {
        assert!(ClassFile::parse(&[]).is_err());
        assert!(ClassFile::parse(&[0xCA, 0xFE, 0xBA, 0xBE]).is_err());

        // 幻数不对。
        assert!(ClassFile::parse(&[0, 1, 2, 3, 0, 0, 0, 52, 0, 1]).is_err());

        // 常量池标签非法。
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&52u16.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.push(99);
        assert!(ClassFile::parse(&bytes).is_err());
    }
}
