/** 通用小图标。 */

/** 斜向右上的箭头（↗）：表示「到外部打开」。
 * 与创意工坊行内「在 Steam 中打开」按钮是同一枚：
 * 不用三点连线的分享图标，也不画成先上后右的折线。
 * 文本按钮里直接放在文字后（`打开 <ExternalArrowIcon />`），
 * 尺寸随字号走（.pz-external-arrow-icon）；工坊方形按钮里传
 * className="pz-workshop-share-icon" 沿用固定 0.95rem 尺寸。 */
export function ExternalArrowIcon({ className = "pz-external-arrow-icon" }: { className?: string }) {
  return (
    <svg aria-hidden="true" className={className} fill="none" viewBox="0 0 16 16">
      <path
        d="M3.4 12.6 12.8 3.2"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="1.6"
      />
      <path
        d="M6.8 3.2H12.8V9.2"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="1.6"
      />
    </svg>
  );
}
