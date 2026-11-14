-- pzm（pzmanager）自动加入服务器：设置 › 实验性功能。
--
-- 架构（2026-11-14）：启动器把 `+connect 主机:端口`（可选 `+password 密码`）程序参数
-- 追加到 java 命令，游戏自己的原生链路完成一切重活——主菜单就绪时游戏触发
-- OnMainMenuEnter → LoadMainScreenPanelInt 读 args.server.connect 属性（读后即清）→
-- BootstrapConnectPopup:connect 查询服务器 → ServerConnectPopup 弹出（地址/密码已预填）。
-- 原生流程只差"点一下连接"，本钩子把它升级为零点击：
--
--   包一层 ServerConnectPopup:setServer —— 弹窗预填完之后，若目标与本次启动的
--   Zomboid\Lua\pzm-autojoin.ini（启动器写，只含 target=）一致，就复刻 CONNECT 按钮
--   的原始逻辑直接发起连接（用户名 Steam 模式无关紧要，空则用当前档案名兜底）。
--
-- 钩子失败（游戏版本改动导致 API 变化等）只损失自动点击：弹窗照常弹出，用户手点即可，
-- 不会阻断连接。全部逻辑 pcall 包裹，绝不在主菜单抛错。
--
-- 注意：本文件由启动器每次启动重写（游戏大版本更新清掉 media 后自愈）；
-- pzopt/ 目录是 PZ_Optimization 的地盘，不要把本文件挪进去。
-- 日志全部用 ASCII：游戏 JVM 按 GBK 读 Lua 源码，中文 print 会变成乱码。

local CONFIG_FILE = "pzm-autojoin.ini"

local state = {
    hooked = false,
    -- 已为本目标自动点过连接：setServer 可能被调多次（重试/手开弹窗），只自动点一次。
    clicked = false,
}

local function trim(value)
    return (value:gsub("^%s*(.-)%s*$", "%1"))
end

-- 读 Zomboid\Lua\pzm-autojoin.ini（B42 的 cacheFileExists/getFileReader 相对路径
-- 解析到该目录）。只关心 target= 行；文件不存在 / 被 consumed 时返回 nil，
-- 钩子就什么都不做。
local function readConfigTarget()
    if not cacheFileExists(CONFIG_FILE) then
        return nil
    end
    local ok, reader = pcall(getFileReader, CONFIG_FILE, false)
    if not ok or not reader then
        return nil
    end
    local target, consumed = nil, false
    while true do
        local line = reader:readLine()
        if line == nil then
            break
        end
        local key, value = line:match("^%s*([%w_]+)%s*=%s*(.-)%s*$")
        if key == "target" then
            target = trim(value)
        elseif key == "consumed" then
            consumed = value ~= ""
        end
    end
    reader:close()
    if consumed then
        return nil
    end
    return target
end

-- 一次性语义：发起自动点击后立刻改写成 consumed=1。启动器中途被杀没来得及清理时，
-- 顶多下一次启动游戏再触发一次，不会反复自动连接。
local function consumeConfig()
    local ok, writer = pcall(getFileWriter, CONFIG_FILE, true, false)
    if not ok or not writer then
        return
    end
    pcall(function() writer:write("consumed=1\r\n") end)
    pcall(function() writer:close() end)
end

-- 目标匹配：ini 的 "主机:端口" 对上弹窗的 ip / port（端口统一按数字比较，
-- 前导零与字符串形式差异不影响）。
local function targetMatches(target, ip, port)
    if not target or not ip or port == nil then
        return false
    end
    local host, configPort = target:match("^(.-):(%d+)$")
    if not host or host ~= ip then
        return false
    end
    return tonumber(configPort) == tonumber(port)
end

-- 复刻 ServerConnectPopup:onOptionMouseDown 的 CONNECT 分支（不经过 checkFields 的
-- 用户名门槛——Steam 模式下用户名无关紧要，服务器只认 Steam ID；非 Steam 模式且
-- 拿不到用户名时保持弹窗等手点，避免用空身份硬连）。
local function autoClickConnect(popup)
    local username = popup.usernameEntry and popup.usernameEntry:getText() or ""
    if trim(username) == "" then
        local steamMode = getSteamModeActive()
        local profile = steamMode and type(getCurrentUserProfileName) == "function"
            and getCurrentUserProfileName() or nil
        if trim(tostring(profile)) ~= "" then
            popup.usernameEntry:setText(profile)
            username = profile
        elseif steamMode then
            username = "SteamPlayer"
            popup.usernameEntry:setText(username)
            print("[pzm] autojoin: no profile name, using placeholder username")
        else
            print("[pzm] autojoin: no username (non-steam mode), leaving popup for manual connect")
            return false
        end
    end
    local useSteamRelay = false
    if getSteamModeActive() and popup.connectTypeEntry
        and popup.connectTypeEntry.selected and popup.connectTypeEntry.selected[1] then
        useSteamRelay = true
    end
    ConnectToServer.instance:connect(popup, "", username, popup.passwordEntry:getInternalText(),
        popup.ip, "", popup.port, popup.serverPasswordEntry:getInternalText(), useSteamRelay, true, 1)
    return true
end

local function hookSetServer()
    if state.hooked or type(ServerConnectPopup) ~= "table"
        or type(ServerConnectPopup.setServer) ~= "function" then
        return
    end
    local original = ServerConnectPopup.setServer
    ServerConnectPopup.setServer = function(self, ip, port, passwordStr)
        local result = original(self, ip, port, passwordStr)
        local ok, error = pcall(function()
            local target = readConfigTarget()
            if state.clicked or not target or not targetMatches(target, ip, port) then
                return
            end
            print("[pzm] autojoin: launcher popup detected, auto-clicking CONNECT -> "
                .. tostring(ip) .. ":" .. tostring(port))
            state.clicked = true
            consumeConfig()
            local clicked = autoClickConnect(self)
            if clicked then
                print("[pzm] autojoin: connect initiated")
            end
        end)
        if not ok then
            print("[pzm] autojoin: auto-click failed (popup kept for manual connect) - "
                .. tostring(error))
        end
        return result
    end
    state.hooked = true
end

Events.OnGameBoot.Add(hookSetServer)
