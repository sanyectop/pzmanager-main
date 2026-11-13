/** 拖动时视觉连续、松开后就近吸附的滑动条。
 *
 * 拖动过程按 1 步进（视觉上没有档位粒度），松手（onValueChangeEnd）后把当前值
 * 吸附到最近的 step 档位；吸附的平滑过渡由 `.pz-memory-slider` 上已有的
 * range / thumb transition 提供，本组件不额外做动画。
 *
 * 键盘可达性：方向键按 1 步进，Shift + 方向键按 largeStep（默认 10 × step，
 * 由 zag 提供）大步移动，键盘用户不需要一格一格挪。
 */
import { Slider } from "@chakra-ui/react";

export function SnapSlider({
  ariaLabel,
  className,
  min,
  max,
  step,
  value,
  disabled,
  onChange,
  onCommit,
}: {
  ariaLabel: string;
  className?: string;
  min: number;
  max: number;
  /** 松开时吸附的档位；拖动过程固定按 1 步进。 */
  step: number;
  value: number;
  /** 禁用时不可拖动也不响应键盘。 */
  disabled?: boolean;
  /** 拖动过程中的细步值（与松开后的吸附值共用同一个回调）。 */
  onChange: (value: number) => void;
  /** 松开后调用，参数是吸附到档位后的值；不吸附（已在档位上）也照常调用。 */
  onCommit?: (value: number) => void;
}) {
  return (
    <Slider.Root
      aria-label={[ariaLabel]}
      className={className}
      disabled={disabled}
      max={max}
      min={min}
      step={1}
      value={[value]}
      onValueChange={({ value }) => {
        const nextValue = value[0];
        if (nextValue !== undefined) {
          onChange(nextValue);
        }
      }}
      onValueChangeEnd={({ value }) => {
        const nextValue = value[0];
        if (nextValue === undefined) {
          return;
        }
        // 就近吸附到 min + k × step 的档位（min 不一定是 step 的整数倍）。
        const snapped = Math.min(
          max,
          Math.max(min, min + Math.round((nextValue - min) / step) * step),
        );
        if (snapped !== nextValue) {
          onChange(snapped);
        }
        onCommit?.(snapped);
      }}
    >
      <Slider.Control>
        <Slider.Track>
          <Slider.Range />
        </Slider.Track>
        <Slider.Thumb index={0}>
          <Slider.HiddenInput />
        </Slider.Thumb>
      </Slider.Control>
    </Slider.Root>
  );
}
