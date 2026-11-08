/**
 * 四季壁纸的粒子特效（移植自参考 HTML 的 canvas 场景）：
 *  - 春：细长的白色光带，向右飘；
 *  - 夏：细雨，快速斜落；
 *  - 秋：五裂枫叶，边落边旋转；
 *  - 冬：雪花光晕，缓慢飘落。
 *
 * 粒子数量按四季混合权重动态平衡（权重来自 `WallpaperEffect.weights`），
 * 所以换季时是"这套粒子淡出、那套淡入"，不会突然切换。
 *
 * 纯 canvas 实现，不依赖 React；组件只负责创建 / 传入权重 / 销毁。
 */

export type ParticleSeasonWeights = Record<"spring" | "summer" | "autumn" | "winter", number>;

const SEASONS = ["spring", "summer", "autumn", "winter"] as const;
type SeasonKey = (typeof SEASONS)[number];

/** 目标粒子总数（跨四季共享配额），和参考实现一致。 */
const TOTAL_PARTICLES = 10;
/** 同屏上限：留一点余量给正在淡出的旧粒子。 */
const MAX_TOTAL = 16;
/** 春季光带的分段数。 */
const SPRING_SEGMENT_COUNT = 14;

const AUTUMN_COLORS = [
  "#c0392b",
  "#d9534f",
  "#e0703a",
  "#e8a13c",
  "#b8452e",
  "#f0b040",
  "#a83a2a",
];

type Particle = {
  type: SeasonKey;
  dying: boolean;
  phase: number;
  a: number;
  maxA: number;
  x: number;
  y: number;
  size?: number;
  len?: number;
  thickness?: number;
  speed?: number;
  amp?: number;
  freq: number;
  phaseSpeed?: number;
  driftY?: number;
  vx?: number;
  vy?: number;
  lw?: number;
  swayAmp?: number;
  rot?: number;
  vr?: number;
  color?: string;
};

/** 五裂枫叶轮廓（极坐标裂片），与参考实现逐点一致。 */
function buildMaplePath(): Path2D {
  const lobes = [
    { a: 0.0, h: 1.0, w: 0.52 },
    { a: 0.92, h: 0.86, w: 0.44 },
    { a: -0.92, h: 0.86, w: 0.44 },
    { a: 1.86, h: 0.6, w: 0.34 },
    { a: -1.86, h: 0.6, w: 0.34 },
  ];
  const base = 0.24;
  const angles: number[] = [];
  const samples = 90;
  for (let i = 0; i < samples; i++) angles.push((i / samples) * Math.PI * 2);
  for (const lobe of lobes) angles.push(lobe.a < 0 ? lobe.a + Math.PI * 2 : lobe.a);
  angles.sort((left, right) => left - right);

  const path = new Path2D();
  let started = false;
  for (const angle of angles) {
    const theta = angle > Math.PI ? angle - Math.PI * 2 : angle;
    let radius = base;
    for (const lobe of lobes) {
      const value = lobe.h * (1 - Math.abs(theta - lobe.a) / lobe.w);
      if (value > radius) radius = value;
    }
    const x = Math.sin(theta) * radius;
    const y = -Math.cos(theta) * radius;
    if (!started) {
      path.moveTo(x, y);
      started = true;
    } else {
      path.lineTo(x, y);
    }
  }
  path.closePath();
  // 叶柄
  path.moveTo(-0.032, 0.18);
  path.lineTo(0.032, 0.18);
  path.lineTo(0.02, 0.5);
  path.lineTo(-0.02, 0.5);
  path.closePath();
  return path;
}

/** 权重最高的季节（并列时取先出现的那个）。 */
function dominantSeason(weights: ParticleSeasonWeights): SeasonKey {
  return SEASONS.reduce((best, season) => ((weights[season] ?? 0) > (weights[best] ?? 0) ? season : best));
}

/** 预渲染的雪花光晕（径向渐变），避免每帧画渐变。 */
function buildSnowSprite(): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 64;
  const ctx = canvas.getContext("2d");
  if (!ctx) return canvas;
  const gradient = ctx.createRadialGradient(32, 32, 0, 32, 32, 32);
  gradient.addColorStop(0, "rgba(255,255,255,1)");
  gradient.addColorStop(0.18, "rgba(255,255,255,0.92)");
  gradient.addColorStop(0.42, "rgba(214,236,255,0.28)");
  gradient.addColorStop(1, "rgba(214,236,255,0)");
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, 64, 64);
  return canvas;
}

/**
 * 创建并启动粒子系统。返回控制句柄：`setWeights` 更新四季权重，`destroy` 停表并解绑。
 */
export function createSeasonalParticles(canvas: HTMLCanvasElement) {
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return { setWeights: () => undefined, destroy: () => undefined };
  }

  const maplePath = buildMaplePath();
  const snowSprite = buildSnowSprite();

  let width = 0;
  let height = 0;
  let dpr = 1;
  let weights: ParticleSeasonWeights = { spring: 1, summer: 0, autumn: 0, winter: 0 };
  let particles: Particle[] = [];
  let frame = 0;
  let lastTime = 0;
  let elapsed = 0;
  let wind = 0;
  let running = true;
  /** 是否已经用真实权重做过一次初始播种。 */
  let seeded = false;
  const spawnAccum: Record<SeasonKey, number> = { spring: 0, summer: 0, autumn: 0, winter: 0 };

  const resize = () => {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    width = canvas.clientWidth || window.innerWidth;
    height = canvas.clientHeight || window.innerHeight;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  };

  const makeParticle = (type: SeasonKey, seeded: boolean): Particle => {
    const particle: Particle = {
      type,
      dying: false,
      phase: Math.random() * Math.PI * 2,
      a: 0,
      maxA: 1,
      x: 0,
      y: 0,
      freq: 1,
    };
    if (type === "spring") {
      particle.x = Math.random() * (width + 600) - 300;
      particle.y = Math.random() * height;
      particle.len = 90 + Math.random() * 180;
      particle.thickness = 0.7 + Math.random() * 1.3;
      particle.speed = 70 + Math.random() * 140;
      particle.amp = 2 + Math.random() * 5;
      particle.freq = 0.8 + Math.random() * 1.6;
      particle.phaseSpeed = 0.5 + Math.random();
      particle.driftY = (Math.random() - 0.5) * 14;
      particle.maxA = 0.1 + Math.random() * 0.2;
    } else if (type === "summer") {
      particle.x = Math.random() * (width + 240) - 120;
      particle.y = Math.random() * height;
      particle.vx = -40 - Math.random() * 60;
      particle.vy = 420 + Math.random() * 460;
      particle.lw = 0.6 + Math.random() * 0.9;
      particle.maxA = 0.1 + Math.random() * 0.32;
    } else if (type === "autumn") {
      particle.x = Math.random() * width;
      particle.y = Math.random() * height;
      particle.size = 8 + Math.random() * 12;
      particle.vy = 28 + Math.random() * 55;
      particle.swayAmp = 18 + Math.random() * 38;
      particle.freq = 0.5 + Math.random() * 1.1;
      particle.rot = Math.random() * Math.PI * 2;
      particle.vr = (Math.random() - 0.5) * 1.8;
      particle.color = AUTUMN_COLORS[Math.floor(Math.random() * AUTUMN_COLORS.length)];
      particle.maxA = 0.75 + Math.random() * 0.25;
    } else {
      particle.x = Math.random() * width;
      particle.y = Math.random() * height;
      particle.size = 1.2 + Math.random() * 2.4;
      particle.vy = 18 + Math.random() * 50;
      particle.swayAmp = 8 + Math.random() * 26;
      particle.freq = 0.4 + Math.random() * 1.4;
      particle.maxA = 0.35 + Math.random() * 0.6;
    }
    particle.a = seeded ? particle.maxA * (0.35 + Math.random() * 0.65) : 0;
    return particle;
  };

  /** 按权重补齐 / 淡出粒子。 */
  const rebalance = (dt: number) => {
    const alive: Record<SeasonKey, number> = { spring: 0, summer: 0, autumn: 0, winter: 0 };
    for (const particle of particles) {
      if (!particle.dying) alive[particle.type]++;
    }
    for (const season of SEASONS) {
      const target = (weights[season] ?? 0) * TOTAL_PARTICLES;
      const diff = alive[season] - target;
      if (diff >= 0.5) {
        let need = Math.round(diff);
        for (let i = particles.length - 1; i >= 0 && need > 0; i--) {
          const particle = particles[i];
          if (particle.type === season && !particle.dying) {
            particle.dying = true;
            alive[season]--;
            need--;
          }
        }
      }
    }
    for (const season of SEASONS) {
      const target = (weights[season] ?? 0) * TOTAL_PARTICLES;
      if (alive[season] < target) {
        spawnAccum[season] += dt * 12;
        while (spawnAccum[season] >= 1 && alive[season] < target && particles.length < MAX_TOTAL) {
          particles.push(makeParticle(season, false));
          alive[season]++;
          spawnAccum[season] -= 1;
        }
      } else {
        spawnAccum[season] = 0;
      }
    }
  };

  const update = (dt: number) => {
    for (let i = particles.length - 1; i >= 0; i--) {
      const particle = particles[i];
      if (particle.dying) {
        particle.a -= dt * 1.5;
        if (particle.a <= 0) {
          particles.splice(i, 1);
          continue;
        }
      } else if (particle.a < particle.maxA) {
        particle.a = Math.min(particle.maxA, particle.a + dt * 1.4);
      }

      if (particle.type === "spring") {
        particle.phase += dt * (particle.phaseSpeed ?? 1);
        particle.x += (particle.speed ?? 100) * (0.6 + wind * 0.8) * dt;
        particle.y += (particle.driftY ?? 0) * dt;
        const half = (particle.len ?? 100) * 0.5;
        if (particle.x - half > width + 100) {
          particle.x = -half - 80 - Math.random() * 150;
          particle.y = Math.random() * height;
        }
        if (particle.y < -60) particle.y = height + 40;
        else if (particle.y > height + 60) particle.y = -40;
      } else if (particle.type === "summer") {
        particle.x += ((particle.vx ?? -40) + wind * 110) * dt;
        particle.y += (particle.vy ?? 420) * dt;
        if (particle.y > height + 24) {
          particle.y = -24 - Math.random() * 80;
          particle.x = Math.random() * (width + 240) - 120;
        }
        if (particle.x < -140) particle.x = width + 120;
        else if (particle.x > width + 140) particle.x = -120;
      } else if (particle.type === "autumn") {
        particle.phase += dt * particle.freq;
        particle.x += (Math.sin(particle.phase) * (particle.swayAmp ?? 0) + wind * 40) * dt;
        particle.y += (particle.vy ?? 30) * dt;
        particle.rot = (particle.rot ?? 0) + (particle.vr ?? 0) * dt;
        if (particle.y > height + 60) {
          particle.y = -60;
          particle.x = Math.random() * width;
        }
        if (particle.x < -80) particle.x = width + 60;
        else if (particle.x > width + 80) particle.x = -60;
      } else {
        particle.phase += dt * particle.freq;
        particle.x += (Math.sin(particle.phase) * (particle.swayAmp ?? 0) + wind * 34) * dt;
        particle.y += (particle.vy ?? 20) * dt;
        if (particle.y > height + 12) {
          particle.y = -12;
          particle.x = Math.random() * width;
        }
        if (particle.x < -24) particle.x = width + 24;
        else if (particle.x > width + 24) particle.x = -24;
      }
    }
  };

  /** 春：矩形截面的光带，沿长度轻微扭曲，头亮尾淡。 */
  const drawSpringStrip = (particle: Particle) => {
    const len = particle.len ?? 100;
    const half = len * 0.5;
    const thickness = particle.thickness ?? 1;
    const amp = particle.amp ?? 2;
    const freq = particle.freq;
    const speedX = (particle.speed ?? 100) * (0.6 + wind * 0.8);
    const angle = Math.atan2(particle.driftY ?? 0, speedX);

    const xs: number[] = [];
    const ys: number[] = [];
    for (let i = 0; i <= SPRING_SEGMENT_COUNT; i++) {
      const t = i / SPRING_SEGMENT_COUNT;
      xs.push(-half + t * len);
      ys.push(Math.sin(particle.phase + t * freq * Math.PI) * amp * Math.sin(Math.PI * t));
    }

    ctx.save();
    ctx.translate(particle.x, particle.y);
    ctx.rotate(angle);
    const halfT = thickness * 0.5;
    const gradient = ctx.createLinearGradient(-half, 0, half, 0);
    gradient.addColorStop(0, "rgba(255,255,255,0)");
    gradient.addColorStop(0.45, "rgba(255,255,255,0.5)");
    gradient.addColorStop(1, "rgba(255,255,255,1)");
    ctx.fillStyle = gradient;

    ctx.beginPath();
    for (let i = 0; i <= SPRING_SEGMENT_COUNT; i++) {
      const prev = i > 0 ? i - 1 : 0;
      const next = i < SPRING_SEGMENT_COUNT ? i + 1 : SPRING_SEGMENT_COUNT;
      let tx = xs[next] - xs[prev];
      let ty = ys[next] - ys[prev];
      const length = Math.hypot(tx, ty) || 1;
      tx /= length;
      ty /= length;
      const px = xs[i] - ty * halfT;
      const py = ys[i] + tx * halfT;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    for (let i = SPRING_SEGMENT_COUNT; i >= 0; i--) {
      const prev = i > 0 ? i - 1 : 0;
      const next = i < SPRING_SEGMENT_COUNT ? i + 1 : SPRING_SEGMENT_COUNT;
      let tx = xs[next] - xs[prev];
      let ty = ys[next] - ys[prev];
      const length = Math.hypot(tx, ty) || 1;
      tx /= length;
      ty /= length;
      ctx.lineTo(xs[i] + ty * halfT, ys[i] - tx * halfT);
    }
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  };

  const draw = () => {
    ctx.clearRect(0, 0, width, height);
    ctx.lineCap = "round";
    ctx.globalCompositeOperation = "source-over";
    for (const particle of particles) {
      if (particle.a <= 0.002) continue;
      ctx.globalAlpha = particle.a > 1 ? 1 : particle.a;
      if (particle.type === "spring") {
        drawSpringStrip(particle);
      } else if (particle.type === "summer") {
        ctx.strokeStyle = "#c9e6ff";
        ctx.lineWidth = particle.lw ?? 1;
        ctx.beginPath();
        ctx.moveTo(particle.x, particle.y);
        ctx.lineTo(particle.x - (particle.vx ?? -40) * 0.04, particle.y - (particle.vy ?? 420) * 0.04);
        ctx.stroke();
      } else if (particle.type === "autumn") {
        const size = particle.size ?? 10;
        ctx.save();
        ctx.translate(particle.x, particle.y);
        ctx.rotate(particle.rot ?? 0);
        ctx.scale(size, size);
        ctx.fillStyle = particle.color ?? "#c0392b";
        ctx.fill(maplePath);
        ctx.strokeStyle = "rgba(70, 18, 8, 0.22)";
        ctx.lineWidth = 0.06;
        ctx.stroke(maplePath);
        ctx.restore();
      } else {
        const size = (particle.size ?? 2) * 5;
        ctx.drawImage(snowSprite, particle.x - size * 0.5, particle.y - size * 0.5, size, size);
      }
    }
    ctx.globalAlpha = 1;
  };

  const loop = (now: number) => {
    if (!running) return;
    if (!lastTime) lastTime = now;
    let dt = (now - lastTime) / 1000;
    lastTime = now;
    if (dt > 0.05) dt = 0.05;
    if (dt < 0) dt = 0;
    elapsed += dt;
    wind = Math.sin(elapsed * 0.37) * 0.7 + Math.sin(elapsed * 0.113 + 1.3) * 0.3;
    rebalance(dt);
    update(dt);
    draw();
    frame = window.requestAnimationFrame(loop);
  };

  const handleResize = () => {
    resize();
  };
  const resizeObserver =
    typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(() => resize());
  const handleVisibility = () => {
    if (document.hidden) {
      running = false;
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
    } else if (!running) {
      running = true;
      lastTime = 0;
      frame = window.requestAnimationFrame(loop);
    }
  };

  resize();
  // 布局有时在创建后一拍才稳定（弹窗打开动画 / 首帧尺寸未定），再量一次。
  window.requestAnimationFrame(() => resize());
  window.addEventListener("resize", handleResize);
  document.addEventListener("visibilitychange", handleVisibility);
  resizeObserver?.observe(canvas);
  // 先给主导季节铺一批"已经在场"的粒子，避免开局空场再逐一冒出来。
  for (let i = 0; i < TOTAL_PARTICLES; i++) {
    particles.push(makeParticle(dominantSeason(weights), true));
  }
  frame = window.requestAnimationFrame(loop);

  return {
    /** 更新四季权重（0–1，通常加起来为 1），换季时粒子自动此消彼长。 */
    setWeights(next: ParticleSeasonWeights) {
      weights = next;
      // 第一次拿到真实权重时按主导季节重建初始粒子，避免开局清一色"春"。
      if (!seeded) {
        seeded = true;
        const dominant = dominantSeason(next);
        if (dominant !== "spring") {
          particles = [];
          for (let i = 0; i < TOTAL_PARTICLES; i++) particles.push(makeParticle(dominant, true));
        }
      }
    },
    /** 调试用：当前存活粒子数（按季节）。 */
    debugCounts() {
      const counts: Record<SeasonKey, number> = { spring: 0, summer: 0, autumn: 0, winter: 0 };
      for (const particle of particles) {
        if (!particle.dying) counts[particle.type]++;
      }
      return { total: particles.length, ...counts };
    },
    destroy() {
      running = false;
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
      window.removeEventListener("resize", handleResize);
      document.removeEventListener("visibilitychange", handleVisibility);
      resizeObserver?.disconnect();
      particles = [];
      ctx.clearRect(0, 0, width, height);
    },
  };
}
