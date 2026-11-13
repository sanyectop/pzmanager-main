/** 内置壁纸主界面的背景音乐播放器。
 *
 *  外观复用「创意工坊壁纸 · Steam 详细信息」的胶囊展开控件：左边一个箭头按钮，
 *  点开向右滑出一块半透明亚克力面板；面板内容改成播放器（曲名 / 播放暂停 / 进度条 / 音量 / 时间）。
 *  音源固定是壁纸目录 `public/backgrounds/main.mp3`（随前端打包，打包后即 `/backgrounds/main.mp3`）。
 *
 *  播放状态（音量 / 是否在播 / 播放位置 / 面板是否展开）通过 `set_launch_music_state`
 *  落到 settings.json，下次启动自动恢复。
 *
 *  播放逻辑放在常驻 App 根部的 `LaunchMusicProvider`，启动页只渲染胶囊控件
 *  `LaunchMusicControls`：切到其它页面时控件随页面卸载，但 `<audio>` 不卸载，
 *  音乐继续播；否则每次切页都会从头开始。
 *
 *  默认自动播放：首次进入（没有存档状态）就直接起播；WebView 按手势策略拒绝时，
 *  等用户第一次点/按键再补放。真正开始播放时通过 `onMusicStarted` 让上层弹一条右上角提示。
 */
import { Box, Button, HStack, Text } from "@chakra-ui/react";
import { invoke } from "@tauri-apps/api/core";
import {
  createContext,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  DEFAULT_LAUNCH_MUSIC_STATE,
  formatMusicTime,
  LAUNCH_MUSIC_TRACK_TITLE,
  LAUNCH_MUSIC_STORAGE_KEY,
  LAUNCH_MUSIC_URL,
  LaunchMusicState,
  MUSIC_VOLUME_DEFAULT,
  MUSIC_AUTOPLAY_RETRY_EVENTS,
  MUSIC_SEEK_STEP_PERCENT,
  MUSIC_VOLUME_STEP,
  musicAudioVolume,
  musicPlayLabel,
  musicProgressPercent,
  musicToggleLabel,
  normalizeLaunchMusicState,
  normalizeMusicVolume,
  persistedPosition,
  MUSIC_PROGRESS_SAVE_INTERVAL_MS,
  pickNewerMusicState,
} from "../launchMusicUi";

/** 播放中每 5 秒落盘一次位置，避免频繁写 settings.json。 */
const POSITION_SAVE_INTERVAL_MS = 5000;

/** 胶囊控件需要的实时状态与操作，由常驻的 `LaunchMusicProvider` 提供。 */
type LaunchMusicControlsValue = {
  open: boolean;
  playing: boolean;
  current: number;
  duration: number | null;
  volume: number;
  failed: boolean;
  progressPercent: number;
  toggleOpen: () => void;
  togglePlay: () => void;
  seekTo: (percent: number) => void;
  applyVolume: (percent: number) => void;
};

const LaunchMusicContext = createContext<LaunchMusicControlsValue | null>(null);

/** 常驻 App 根部的播放器：持有 `<audio>` 与全部播放状态，切页不会中断播放。 */
export function LaunchMusicProvider({
  enabled = true,
  onMusicStarted,
  pauseSignal = 0,
  children,
}: {
  /** 当前是否是内置四季壁纸；切到自定义壁纸时音乐应当停下。 */
  enabled?: boolean;
  /** 自动播放真正起播时回调一次（上层用它弹右上角提示）。 */
  onMusicStarted?: (message: string) => void;
  /** 每次自增表示上层要求暂停（如游戏启动完成）。0 = 从未触发。 */
  pauseSignal?: number;
  children?: ReactNode;
}) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [ready, setReady] = useState(false);
  const [open, setOpen] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState<number | null>(null);
  const [volume, setVolume] = useState(MUSIC_VOLUME_DEFAULT);
  const [failed, setFailed] = useState(false);

  /** 最近一次要落盘的状态（ref 里保存，供卸载 / 定时器读到最新值）。 */
  const stateRef = useRef<LaunchMusicState>({ ...DEFAULT_LAUNCH_MUSIC_STATE });
  const saveTimerRef = useRef<number | null>(null);
  /** 本次会话是否已经提示过"开始播放"：自动播放重试成功也只提示一次。 */
  const startedNotifiedRef = useRef(false);
  /** 上层回调放 ref 里，避免把它塞进 effect 依赖导致自动播放被反复触发。 */
  const onMusicStartedRef = useRef(onMusicStarted);
  /** `enabled` 的实时值：自动播放 / 补放的回调里要读它，但不想为此重建回调。 */
  const enabledRef = useRef(enabled);
  /** 存档里的播放位置是否还没真正应用到 <audio> 上。
   *  未应用前绝不能把它覆盖成 audio.currentTime（那时还是 0），否则每次启动都从头播。 */
  const restorePendingRef = useRef(false);
  /** 存档里待恢复的播放位置（0 = 没有待恢复项）。 */
  const restoreTargetRef = useRef(0);
  /** 是否已经成功读到过存档。
   *  没读到之前绝不能落盘：此时 stateRef 还是默认值（playing=true / position=0），
   *  写回去就把上次的"暂停 + 进度"覆盖了——用户反馈的"暂停和进度没保存"
   *  就是组件在读档完成前被卸载/刷新触发了一次默认值写入。 */
  const loadedRef = useRef(false);
  onMusicStartedRef.current = onMusicStarted;
  enabledRef.current = enabled;

  const progressPercent = useMemo(
    () => musicProgressPercent(current, duration),
    [current, duration],
  );

  /** 只写本地缓存（同步），返回写入的快照。IPC 由调用方决定要不要发。 */
  const writeLocalSnapshot = useCallback((): LaunchMusicState => {
    const snapshot: LaunchMusicState = { ...stateRef.current, savedAt: Date.now() };
    stateRef.current.savedAt = snapshot.savedAt;
    try {
      window.localStorage.setItem(LAUNCH_MUSIC_STORAGE_KEY, JSON.stringify(snapshot));
    } catch {
      // 隐私模式 / 配额满：忽略。
    }
    return snapshot;
  }, []);

  /** 把当前播放进度写进本地缓存（同步、便宜）；settings.json 仍走 IPC 的 5 秒节流。 */
  const persistProgressNow = useCallback(() => {
    if (!loadedRef.current || restorePendingRef.current) return;
    const audio = audioRef.current;
    if (!audio || audio.paused) return;
    stateRef.current.playing = true;
    stateRef.current.positionSec = audio.currentTime;
    writeLocalSnapshot();
  }, [writeLocalSnapshot]);

  /** 把 stateRef 里的状态写进 settings.json（合并成一次调用，忽略失败）。 */
  const flushState = useCallback(() => {
    // 还没读到存档：这一拍写的只是默认值，写下去等于把用户的上次状态抹掉。
    if (!loadedRef.current) return;
    if (saveTimerRef.current != null) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    const audio = audioRef.current;
    // playing 以 <audio> 的实时状态为准：play/pause 事件可能乱序（快速点两次时后到的
    // play 事件会把刚写进去的 false 再改回 true），落盘时必须反映"现在到底在不在放"。
    if (audio) {
      stateRef.current.playing = !audio.paused;
    }
    stateRef.current.positionSec = persistedPosition({
      restorePending: restorePendingRef.current,
      audioTime: audio ? audio.currentTime : null,
      storedPosition: stateRef.current.positionSec,
    });
    // 先同步写本地缓存：刷新 / 关窗瞬间 `invoke` 可能来不及到后端（实测暂停状态就这样丢过），
    // localStorage 的写入是同步的，下一次启动会按 savedAt 取新的那份。
    const snapshot = writeLocalSnapshot();
    void invoke("set_launch_music_state", { state: snapshot }).catch(() => undefined);
  }, [writeLocalSnapshot]);

  /** 延迟落盘（音量拖动、播放位置这种高频变化用）。 */
  const scheduleSave = useCallback(
    (delayMs: number) => {
      // delayMs = 0 表示"立刻落盘"（播放/暂停/音量/展开这类用户动作），
      // 不能被正在等待的定时器吞掉，所以直接写。
      if (delayMs <= 0) {
        flushState();
        return;
      }
      if (saveTimerRef.current != null) return;
      saveTimerRef.current = window.setTimeout(() => {
        saveTimerRef.current = null;
        flushState();
      }, delayMs);
    },
    [flushState],
  );

  /** 起播成功后提示一次（自动播放 / 手势补放都只提示一次）。 */
  const notifyStarted = useCallback(() => {
    if (startedNotifiedRef.current) return;
    startedNotifiedRef.current = true;
    onMusicStartedRef.current?.(`背景音乐：${LAUNCH_MUSIC_TRACK_TITLE}  可在自定义壁纸设置中取消`);
  }, []);

  /** 尝试起播；被手势策略拒绝时安静失败，交给第一次用户交互补放。 */
  const attemptAutoplay = useCallback(() => {
    // 当前不是内置壁纸：音乐不该响（播放器常驻后不能再靠卸载来挡住）。
    if (!enabledRef.current) return;
    const audio = audioRef.current;
    if (!audio || !audio.paused) return;
    // 用户上次是手动暂停的，就不要自动起播了。
    if (!stateRef.current.playing) return;
    const result = audio.play();
    if (!result) return;
    result.then(() => notifyStarted()).catch(() => setPlaying(false));
  }, [notifyStarted]);

  /** 把存档里的播放位置应用到 <audio> 上（只成功应用一次）。
   *
   *  不能依赖 `duration` 有限：Tauri 资源协议下 `duration` 常常先是 Infinity，
   * 按"duration 必须有限"判断会让定位被永久跳过，表现就是"每次刷新都从 0:00 开始"。 */
  const applyRestoredPosition = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    // 没有存档位置、或元数据还没就绪（此时写 currentTime 会被忽略）。
    if (restoreTargetRef.current <= 0 || audio.readyState < 1) return;
    const target = restoreTargetRef.current;
    restoreTargetRef.current = 0;
    restorePendingRef.current = false;
    const limit =
      Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration - 1 : target;
    audio.currentTime = Math.min(target, Math.max(0, limit));
    stateRef.current.positionSec = audio.currentTime;
    setCurrent(audio.currentTime);
    void invoke("log_frontend_message", {
      level: "info",
      message: `背景音乐：已恢复播放位置 ${audio.currentTime.toFixed(2)}s`,
    }).catch(() => undefined);
  }, []);

  // 元数据就绪的任意一个时机都尝试一次（不同环境下先到的事件不一样）。
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    const handler = () => applyRestoredPosition();
    // 覆盖面尽量宽：不同内核/资源协议下"能定位"的时机不一样（Tauri 资源协议常见
    // duration 先是 Infinity）。`play` / `timeupdate` 兜最后一道——定位只会应用一次，
    // 应用后目标即清零，所以多监听不会造成跳来跳去。
    const events = [
      "loadedmetadata",
      "durationchange",
      "loadeddata",
      "canplay",
      "play",
      "timeupdate",
    ] as const;
    for (const eventName of events) {
      audio.addEventListener(eventName, handler);
    }
    return () => {
      for (const eventName of events) {
        audio.removeEventListener(eventName, handler);
      }
    };
  }, [applyRestoredPosition]);

  // 启动时读取上次的状态：音量 / 播放位置 / 是否续播 / 面板展开。
  useEffect(() => {
    let active = true;
    invoke<{ launchMusic?: unknown }>("get_app_settings")
      .then((settings) => {
        if (!active) return;
        // settings.json 可能落后于"刷新瞬间"的本地缓存，两份按 savedAt 取新的。
        let localState: LaunchMusicState | null = null;
        try {
          const raw = window.localStorage.getItem(LAUNCH_MUSIC_STORAGE_KEY);
          if (raw) localState = normalizeLaunchMusicState(JSON.parse(raw));
        } catch {
          localState = null;
        }
        const fromFile = normalizeLaunchMusicState(settings?.launchMusic);
        const restored = pickNewerMusicState(fromFile, localState);
        // 本地缓存更新（说明上次刷新把 IPC 写丢了）：补一次写回，让 settings.json 追上。
        if (localState && restored === localState) {
          void invoke("set_launch_music_state", { state: restored }).catch(() => undefined);
        }
        stateRef.current = { ...restored };
        restorePendingRef.current = restored.positionSec > 0;
        restoreTargetRef.current = restored.positionSec;
        setVolume(restored.volume);
        setOpen(restored.open);
        const audio = audioRef.current;
        if (audio) {
          audio.volume = musicAudioVolume(restored.volume);
        }
        void invoke("log_frontend_message", {
          level: "info",
          message: `背景音乐：读取存档 playing=${restored.playing} position=${restored.positionSec.toFixed(2)}s volume=${restored.volume}`,
        }).catch(() => undefined);
        // 标记"已读档"：从这里开始，落盘写的才是真实状态。
        loadedRef.current = true;
        // 元数据可能早于这一步就绪，先试一次；没就绪则由事件监听补上。
        applyRestoredPosition();
        // 自动播放：默认就播；被拒绝时由下面的"首次交互补放"兜底。
        if (restored.playing) {
          attemptAutoplay();
        }
        setReady(true);
      })
      .catch(() => {
        if (active) setReady(true);
      });
    return () => {
      active = false;
    };
  }, [attemptAutoplay, applyRestoredPosition]);

  // 换成自定义壁纸（enabled=false）时把音乐停下：Provider 常驻后不能再靠卸载停止播放。
  // 暂停会走 <audio> 的 onPause，暂停位置和状态照常落盘，下次启动不会自己响。
  useEffect(() => {
    if (enabled) return;
    const audio = audioRef.current;
    if (!audio || audio.paused) return;
    audio.pause();
  }, [enabled]);

  // 自动播放被 WebView 拦下时，等用户第一次点击 / 按键补放（成功同样提示一次）。
  useEffect(() => {
    if (!ready) return;
    const handler = () => {
      if (!stateRef.current.playing) return;
      attemptAutoplay();
    };
    for (const eventName of MUSIC_AUTOPLAY_RETRY_EVENTS) {
      window.addEventListener(eventName, handler, { once: true });
    }
    return () => {
      for (const eventName of MUSIC_AUTOPLAY_RETRY_EVENTS) {
        window.removeEventListener(eventName, handler);
      }
    };
  }, [ready, attemptAutoplay]);



  // 播放中每 300ms 把进度写进本地缓存：刷新 / 关窗最多丢 0.3 秒。
  // 用独立定时器而不是 timeupdate（后者约 250ms 一次，节流后实际间隔会变成 0.5 秒）。
  useEffect(() => {
    if (!playing) return;
    persistProgressNow();
    const timer = window.setInterval(() => persistProgressNow(), MUSIC_PROGRESS_SAVE_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [playing, persistProgressNow]);

  // 播放中定期把位置同步给 settings.json；卸载 / 页面隐藏时立刻落盘，避免关窗丢进度。
  useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => {
      stateRef.current.positionSec = audioRef.current?.currentTime ?? stateRef.current.positionSec;
      scheduleSave(POSITION_SAVE_INTERVAL_MS);
    }, POSITION_SAVE_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [playing, scheduleSave]);

  useEffect(() => {
    const handleHide = () => flushState();
    window.addEventListener("pagehide", handleHide);
    window.addEventListener("beforeunload", handleHide);
    return () => {
      window.removeEventListener("pagehide", handleHide);
      window.removeEventListener("beforeunload", handleHide);
      flushState();
    };
  }, [flushState]);

  const syncDuration = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    // 流式加载时 duration 可能是 Infinity / NaN，按未知处理。
    setDuration(Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : null);
  }, []);

  const toggleOpen = useCallback(() => {
    setOpen((value) => {
      const next = !value;
      stateRef.current.open = next;
      scheduleSave(0);
      return next;
    });
  }, [scheduleSave]);

  // 上层要求暂停（游戏启动完成、界面输出「启动用时 XXXs」）：暂停并立刻落盘，
  // 下次启动仍保持暂停，不会自己又响起来。
  useEffect(() => {
    if (pauseSignal <= 0) return;
    const audio = audioRef.current;
    if (!audio || audio.paused) return;
    audio.pause();
    stateRef.current.playing = false;
    const pausedAt = audio.currentTime;
    if (Number.isFinite(pausedAt)) {
      stateRef.current.positionSec = pausedAt;
    }
    restorePendingRef.current = false;
    restoreTargetRef.current = 0;
    scheduleSave(0);
    void invoke("log_frontend_message", {
      level: "info",
      message: `背景音乐：启动完成，自动暂停于 ${pausedAt.toFixed(2)}s`,
    }).catch(() => undefined);
  }, [pauseSignal, scheduleSave]);

  const togglePlay = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    if (audio.paused) {
      const result = audio.play();
      if (result) {
        result.catch(() => setFailed(true));
      }
    } else {
      audio.pause();
    }
  }, []);

  const seekTo = useCallback(
    (percent: number) => {
      const audio = audioRef.current;
      if (!audio || duration == null) return;
      audio.currentTime = (Math.min(100, Math.max(0, percent)) / 100) * duration;
      setCurrent(audio.currentTime);
      stateRef.current.positionSec = audio.currentTime;
      // 用户自己拖过进度，存档位置就不再"待恢复"了。
      restorePendingRef.current = false;
      restoreTargetRef.current = 0;
      scheduleSave(0);
    },
    [duration, scheduleSave],
  );

  const applyVolume = useCallback(
    (percent: number) => {
      const next = normalizeMusicVolume(percent);
      setVolume(next);
      stateRef.current.volume = next;
      const audio = audioRef.current;
      if (audio) audio.volume = musicAudioVolume(next);
      scheduleSave(0);
    },
    [scheduleSave],
  );

  const controls = useMemo<LaunchMusicControlsValue>(
    () => ({
      open,
      playing,
      current,
      duration,
      volume,
      failed,
      progressPercent,
      toggleOpen,
      togglePlay,
      seekTo,
      applyVolume,
    }),
    [
      open,
      playing,
      current,
      duration,
      volume,
      failed,
      progressPercent,
      toggleOpen,
      togglePlay,
      seekTo,
      applyVolume,
    ],
  );

  return (
    <LaunchMusicContext.Provider value={controls}>
      {children}
      <audio
        loop
        onDurationChange={syncDuration}
        onError={() => setFailed(true)}
        onLoadedMetadata={syncDuration}
        onPause={() => {
          setPlaying(false);
          stateRef.current.playing = false;
          // 暂停点就是下次续播点：取音频真实位置，并取消"待恢复"（用户已经给出明确位置）。
          const pausedAt = audioRef.current?.currentTime;
          if (pausedAt != null && Number.isFinite(pausedAt)) {
            stateRef.current.positionSec = pausedAt;
          }
          restorePendingRef.current = false;
          restoreTargetRef.current = 0;
          scheduleSave(0);
          void invoke("log_frontend_message", {
            level: "info",
            message: `背景音乐：暂停于 ${stateRef.current.positionSec.toFixed(2)}s，已保存暂停状态`,
          }).catch(() => undefined);
        }}
        onPlay={() => {
          setPlaying(true);
          setFailed(false);
          stateRef.current.playing = true;
          scheduleSave(0);
        }}
        onTimeUpdate={(event) => setCurrent(event.currentTarget.currentTime)}
        preload="metadata"
        ref={audioRef}
        src={LAUNCH_MUSIC_URL}
        style={{ display: "none" }}
      />
    </LaunchMusicContext.Provider>
  );
}

/** 启动页上的胶囊控件；切页时随页面卸载，音乐由常驻的 Provider 继续播。 */
export function LaunchMusicControls() {
  const music = useContext(LaunchMusicContext);
  if (!music) return null;
  const {
    open,
    playing,
    current,
    duration,
    volume,
    failed,
    progressPercent,
    toggleOpen,
    togglePlay,
    seekTo,
    applyVolume,
  } = music;

  return (
    <Box
      className={`pz-launch-workshop-info pz-launch-music${open ? " is-open" : ""}`}
      data-pz-acrylic="workshop"
    >
      <Box
        aria-hidden={!open}
        className="pz-launch-workshop-info-details pz-launch-music-details"
        id="pz-launch-music-player"
      >
        <Text className="pz-launch-music-title" title={LAUNCH_MUSIC_TRACK_TITLE}>
          <Text as="span" className="pz-launch-music-label">
            背景音乐
          </Text>
          <Text as="span" className="pz-launch-music-track">
            万里升平 - 铁痕电台
          </Text>
        </Text>
        <HStack align="center" className="pz-launch-music-controls" gap="2">
          <Button
            aria-label={musicPlayLabel(playing)}
            className="pz-launch-music-play"
            onClick={togglePlay}
            title={failed ? "音源加载失败，点此重试" : playing ? "暂停" : "播放"}
            variant="ghost"
          >
            <span aria-hidden="true">{playing ? "❚❚" : "▶"}</span>
          </Button>
          <Box
            aria-label="播放进度"
            aria-valuemax={100}
            aria-valuemin={0}
            aria-valuenow={Math.round(progressPercent)}
            className="pz-launch-music-progress"
            onClick={(event) => {
              const rect = event.currentTarget.getBoundingClientRect();
              if (rect.width > 0) {
                seekTo(((event.clientX - rect.left) / rect.width) * 100);
              }
            }}
            role="slider"
            tabIndex={0}
            onKeyDown={(event) => {
              if (event.key === "ArrowLeft") seekTo(progressPercent - MUSIC_SEEK_STEP_PERCENT);
              else if (event.key === "ArrowRight") seekTo(progressPercent + MUSIC_SEEK_STEP_PERCENT);
            }}
          >
            <Box className="pz-launch-music-progress-fill" style={{ width: `${progressPercent}%` }} />
          </Box>
          <Text className="pz-launch-music-time">
            {formatMusicTime(current)} / {formatMusicTime(duration)}
          </Text>
        </HStack>
        <HStack align="center" className="pz-launch-music-volume-row" gap="2">
          <Button
            aria-label={volume === 0 ? "取消静音" : "静音"}
            className="pz-launch-music-mute"
            onClick={() => applyVolume(volume === 0 ? MUSIC_VOLUME_DEFAULT : 0)}
            title={volume === 0 ? "取消静音" : "静音"}
            variant="ghost"
          >
            <span aria-hidden="true">{volume === 0 ? "🔇" : volume < 50 ? "🔉" : "🔊"}</span>
          </Button>
          <input
            aria-label="背景音乐音量"
            className="pz-launch-music-volume"
            max={100}
            min={0}
            onChange={(event) => applyVolume(Number(event.target.value))}
            onKeyDown={(event) => {
              if (event.key === "ArrowLeft") applyVolume(volume - MUSIC_VOLUME_STEP);
              else if (event.key === "ArrowRight") applyVolume(volume + MUSIC_VOLUME_STEP);
            }}
            step={MUSIC_VOLUME_STEP}
            type="range"
            value={volume}
          />
          <Text className="pz-launch-music-volume-value">{volume}</Text>
        </HStack>
      </Box>
      <Button
        aria-controls="pz-launch-music-player"
        aria-expanded={open}
        aria-label={musicToggleLabel(open)}
        className="pz-launch-workshop-info-toggle pz-launch-music-toggle"
        onClick={toggleOpen}
        title={musicToggleLabel(open)}
        variant="ghost"
      >
        <span aria-hidden="true" className="pz-launch-workshop-info-chevron">›</span>
      </Button>
    </Box>
  );
}
