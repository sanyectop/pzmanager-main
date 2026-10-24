import { useState, type ImgHTMLAttributes } from "react";

type FallbackImageProps = Omit<ImgHTMLAttributes<HTMLImageElement>, "src" | "onError"> & {
  src: string;
  /** 主地址加载失败后尝试的备用地址（如官方源），为空时直接显示占位。 */
  fallbackSrc?: string | null;
};

/** 记录某个 src 当前走到了哪一步：primary → fallback → failed。 */
type LoadStage = { src: string; stage: "fallback" | "failed" };

/**
 * 绘制的“图片加载失败”图标：圆角画框 + 太阳 + 山峦，左上角一道裂纹。
 * 全部用 currentColor，颜色跟随所在容器。
 */
function BrokenImageIcon() {
  return (
    <svg
      aria-hidden="true"
      fill="none"
      focusable="false"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.6"
      viewBox="0 0 24 24"
    >
      <rect height="17" rx="3" width="17" x="3.5" y="3.5" />
      <circle cx="8.5" cy="8.5" r="1.6" />
      <path d="M3.5 17 L9 11.5 L12 14.5 L15.5 12 L20.5 16.5" />
      <path d="M14.5 3.5 L13 6.5 L15.5 8.5 L14 10.5" />
    </svg>
  );
}

/**
 * 带加载失败兜底的 <img>。主地址失败时先换用 fallbackSrc（PCL2 的 MyImage 同样
 * 先试镜像再试官方源）；两者都失败才渲染同尺寸的占位图标，不留破图或空白框。
 *
 * 状态按具体 src 记录：换了地址会从头开始，不会沿用上一张图的失败结果。
 */
export function FallbackImage({ src, fallbackSrc, alt = "", className, ...rest }: FallbackImageProps) {
  const [loadStage, setLoadStage] = useState<LoadStage | null>(null);
  const stage = loadStage?.src === src ? loadStage.stage : null;
  const hasFallback = !!fallbackSrc && fallbackSrc !== src;

  if (stage === "failed" || (stage === "fallback" && !hasFallback)) {
    return (
      <span
        aria-label={alt || "图片加载失败"}
        className={className ? `pz-image-fallback ${className}` : "pz-image-fallback"}
        role="img"
        title="图片加载失败"
      >
        <BrokenImageIcon />
      </span>
    );
  }

  const usingFallback = stage === "fallback";
  return (
    <img
      {...rest}
      alt={alt}
      className={className}
      onError={() => {
        if (!usingFallback && hasFallback) {
          setLoadStage({ src, stage: "fallback" });
        } else {
          setLoadStage({ src, stage: "failed" });
        }
      }}
      src={usingFallback && fallbackSrc ? fallbackSrc : src}
    />
  );
}
