# 启动器日志

启动器把关键事件追加到：

```text
%USERPROFILE%\Zomboid\.pzmanager\logs\launcher.log
```

默认只记录启动器自己的事件，不包含游戏内部日志（游戏日志仍看 `Zomboid\console.txt`）。

## 记录内容

- 启动器启动：版本、系统、架构、日志路径。
- 游戏目录检测结果。
- 启动游戏：内存、加载器方案、极速启动开关、启动参数、启动配置写入结果、
  Steam 启动请求成功/失败。
- 版本隔离操作：快照、捕获、还原、校验的开始/完成/错误。
- PZ Optimization 安装/卸载：使用的安装器路径、游戏目录、完成结果。
- 前端未捕获异常（`window.onerror` / `unhandledrejection`）和错误 Toast。
- 反馈提交结果（编号与上传的日志文件名）。

每行格式：

```text
2026-10-29 00:15:32Z [INFO] 检测游戏目录：H:\SteamLibrary\steamapps\common\ProjectZomboid
```

## 轮转与会话

- 单文件超过 **2 MB** 自动轮转，保留 `launcher.log.1` ~ `.3`，最多约 8 MB。
- 每次启动写一行 `=== launcher session ===` 作为会话标记。
- 提交反馈时附带**整份** launcher.log 的尾部（最多 1 MB）。日志按 2 MB 轮转，这 1 MB
  尾部可能包含更早的会话——用户常常是重启启动器之后才来反馈，只带"最近一次会话"
  会把出问题的那一次丢掉，所以现在带上整份日志的尾部。

## 设置 › 日志

- 「刷新」读取日志尾部（默认 256 KB），大文件会提示“仅显示末尾”。
- 「复制」把当前显示内容写入剪贴板。
- 「打开日志文件夹」用资源管理器定位到 `logs` 目录。
- 「清空日志」删除当前内容，并写入一条“日志已清空”记录。

日志只保存在本机；只有用户在「设置 › 关于 › 反馈信息」主动提交反馈时，
launcher.log 的尾部 1 MB（整份日志尾部，可能包含更早的会话）才会随反馈上传。
日志中可能包含本机路径和用户名，提交前请确认。

## 实现

- [src-tauri/src/launcher_log.rs](../src-tauri/src/launcher_log.rs)：追加写、轮转、尾部读取、
  会话切片、测试。
- [src-tauri/src/feedback.rs](../src-tauri/src/feedback.rs)：把 `launcher.log` 加入反馈附件。
- [src/App.tsx](../src/App.tsx)：设置 › 日志页面、前端错误上报。
- 命令：`read_launcher_log`、`clear_launcher_log`、`open_launcher_log_folder`、
  `log_frontend_message`。
