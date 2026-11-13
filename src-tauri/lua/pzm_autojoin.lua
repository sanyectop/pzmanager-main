-- pzm（pzmanager）自动加入服务器：设置 › 实验性功能。
--
-- 启动器每次启动游戏前把一次性配置写进 Zomboid\Lua\pzm-autojoin.ini
-- （target=主机:端口，password=可选；B42 的 cacheFileExists/getFileReader 相对路径
-- 解析到 Zomboid\Lua\，不是 Zomboid 根目录），本钩子在前端（主菜单）就绪后：
--   1. 读一次配置并立刻把文件改写成 consumed=1 —— 启动器中途被杀没来得及清理时，
--      顶多下一次启动游戏再触发一次，不会反复自动连接；
--   2. 在游戏收藏夹里按 ip:端口 找同一台服务器 —— 收藏里保存过密码就用收藏的
--      （用户在游戏里保存的通常比启动器里填的新），否则用配置里的密码；
--      Steam 中继开关同样以收藏条目为准，收藏里没有按直连处理；
--   3. 直接调用 ConnectToServer:connect —— 与直连弹窗「连接」按钮完全相同的调用。
--
-- 装进 <游戏目录>/media/lua/client/ 的散装 Lua 会被游戏自动加载，无需启用任何模组。
-- 游戏大版本更新会清掉 media 里的改动：启动器每次启动都会重写本文件，自愈。
-- 全部逻辑 pcall 包裹：游戏版本改动导致 API 变化时静默失效，不影响正常启动。

local CONFIG_FILE = "pzm-autojoin.ini"

local state = {
    checked = false,
    armed = false,
    ip = nil,
    port = nil,
    password = nil,
}

local function trim(value)
    return (value:gsub("^%s*(.-)%s*$", "%1"))
end

-- 只支持 主机:端口（与游戏主菜单的直连解析一致，IPv6 不支持）。
-- 主机里不允许空白字符；端口必须是 1–65535 的纯数字。
local function splitTarget(target)
    local host, port = target:match("^(.-):(%d+)$")
    if not host or host == "" or host:find("%s") then
        return nil
    end
    local portNumber = tonumber(port)
    if not portNumber or portNumber < 1 or portNumber > 65535 then
        return nil
    end
    return host, port
end

local function parseConfig()
    if not cacheFileExists(CONFIG_FILE) then
        return nil
    end
    local ok, reader = pcall(getFileReader, CONFIG_FILE, false)
    if not ok or not reader then
        return nil
    end
    local config = {}
    while true do
        local line = reader:readLine()
        if line == nil then
            break
        end
        local key, value = line:match("^%s*([%w_]+)%s*=%s*(.-)%s*$")
        if key then
            config[key] = value
        end
    end
    reader:close()
    return config
end

-- 一次性语义：读过就把文件改写成 consumed=1。
local function consumeConfig()
    local ok, writer = pcall(getFileWriter, CONFIG_FILE, true, false)
    if not ok or not writer then
        return
    end
    pcall(function() writer:write("consumed=1\r\n") end)
    pcall(function() writer:close() end)
end

-- 收藏夹里按 ip:端口 找同一台服务器；没有返回 nil。
local function findFavoriteServer(ip, port)
    if type(getServerList) ~= "function" then
        return nil
    end
    local ok, servers = pcall(getServerList)
    if not ok or type(servers) ~= "table" then
        return nil
    end
    for _, server in ipairs(servers) do
        if server:getIp() == ip and tostring(server:getPort()) == tostring(port) then
            return server
        end
    end
    return nil
end

local function tick()
    -- 配置只在本次启动检查一次：启动器在游戏起来之前就把文件写好了。
    if not state.checked then
        state.checked = true
        local config = parseConfig()
        if not config or not config.target or config.consumed then
            return
        end
        local ip, port = splitTarget(trim(config.target))
        if not ip then
            print("[pzm] autojoin: 配置里的服务器地址无效：" .. tostring(config.target))
            consumeConfig()
            return
        end
        state.ip = ip
        state.port = port
        state.password = config.password
        state.armed = true
        consumeConfig()
        print("[pzm] autojoin: 已就绪，等待主菜单 -> " .. ip .. ":" .. port)
    end

    if not state.armed then
        return
    end

    -- 任何"条件不成立"且继续等也没有意义的分支都要解除武装：
    -- 不能在用户手动进服 / 存档界面时突然抢连。
    if type(isDemo) == "function" and isDemo() then
        state.armed = false
        return
    end
    if type(isIngameState) == "function" and isIngameState() then
        state.armed = false
        return
    end

    local mainscreen = MainScreen and MainScreen.instance
    if not mainscreen or mainscreen.inGame then
        return
    end
    local panel = mainscreen.bottomPanel
    if not panel or not panel:getIsVisible() then
        return
    end

    local connect = ConnectToServer and ConnectToServer.instance
    if not connect then
        return
    end
    if connect.connecting or isClient() then
        state.armed = false
        print("[pzm] autojoin: 检测到已在连接，放弃自动加入")
        return
    end

    state.armed = false
    local ok, error = pcall(function()
        local server = findFavoriteServer(state.ip, state.port)
        local username = ""
        local relay = false
        local serverPassword = state.password
        if server then
            username = server:getUserName() or ""
            relay = server:getUseSteamRelay() == true
            local saved = server:getServerPassword()
            if saved and saved ~= "" then
                serverPassword = saved
            end
        end
        print("[pzm] autojoin: 连接 " .. state.ip .. ":" .. state.port ..
            (server and "（使用收藏夹条目）" or "（未在收藏夹，使用启动器配置）"))
        connect:connect(panel, "", username, "", state.ip, "", state.port,
            serverPassword or "", relay, true, 1)
    end)
    if not ok then
        print("[pzm] autojoin: 连接失败 - " .. tostring(error))
    end
end

-- OnFETick 只在前端（主菜单）跑；OnGameBoot 兜底做第一次检查。
Events.OnFETick.Add(function() pcall(tick) end)
Events.OnGameBoot.Add(function() pcall(tick) end)
