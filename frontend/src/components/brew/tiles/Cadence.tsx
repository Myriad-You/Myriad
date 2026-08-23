/**
 * 节律图：一个源的发布密度。
 *
 * 窗口 730 天。x 轴按 `1 - sqrt(days / 730)` 压缩 —— 右端是今天，近期更疏、
 * 远期更密，这样「最近半年只发了三篇」一眼就能看出来，而线性轴会把两年前的
 * 高产期和上个月的沉寂画成一样密。
 *
 * 高度用 `reading_time`；`pulses` 只带天数，所以退回按天数派生的稳定伪随机 ——
 * 同一个源每次渲染都是同一张图，不会闪。
 *
 * 除横轴外不画任何装饰线。
 */

import { memo, useMemo } from 'react'

/** 与后端 `PULSE_WINDOW_DAYS` 一致，改一边要改另一边。 */
export const CADENCE_WINDOW_DAYS = 730

/** 竖线宽（px，未缩放） */
const BAR_WIDTH = 1.5

/** 由天数派生的稳定伪随机高度比（0.35..1）。同一天数永远同一高度。 */
function pseudoHeight(days: number): number {
  const v = Math.sin(days * 12.9898 + 78.233) * 43758.5453
  return 0.35 + (v - Math.floor(v)) * 0.65
}

export interface CadenceProps {
  /** 距今天数，新→旧 */
  pulses: readonly number[]
  color: string
  /** 图高（px，已缩放） */
  height: number
  /** 每根线的高度比来源；缺失时用天数派生的稳定伪随机 */
  readingTimes?: readonly (number | null | undefined)[]
  className?: string
}

export const Cadence = memo(
  ({ pulses, color, height, readingTimes, className }: CadenceProps) => {
    const bars = useMemo(() => {
      return pulses
        .filter((d) => Number.isFinite(d) && d >= 0 && d <= CADENCE_WINDOW_DAYS)
        .map((days, i) => {
          // 右端 = 今天。sqrt 压缩让近期展开、远期收拢。
          const x = (1 - Math.sqrt(days / CADENCE_WINDOW_DAYS)) * 100
          const minutes = readingTimes?.[i]
          const ratio =
            typeof minutes === 'number' && minutes > 0
              ? Math.min(1, 0.35 + minutes / 18)
              : pseudoHeight(days)
          // 近处更不透明：越靠右越是「现在还有没有在更新」的信息
          const opacity = 0.28 + (1 - days / CADENCE_WINDOW_DAYS) * 0.62
          return { x, ratio, opacity, days }
        })
    }, [pulses, readingTimes])

    if (bars.length === 0) return null

    return (
      <div
        className={`relative w-full ${className ?? ''}`}
        style={{ height }}
        aria-hidden
      >
        {bars.map((bar, i) => (
          <span
            key={`${bar.days}-${i}`}
            className="absolute bottom-0 rounded-full"
            style={{
              left: `${bar.x}%`,
              width: BAR_WIDTH,
              height: `${Math.round(bar.ratio * 100)}%`,
              background: color,
              opacity: bar.opacity,
              transform: 'translateX(-50%)',
            }}
          />
        ))}
        {/* 横轴：唯一允许的那条线 */}
        <span
          className="absolute inset-x-0 bottom-0 h-px"
          style={{ background: color, opacity: 0.18 }}
        />
      </div>
    )
  },
)

Cadence.displayName = 'Cadence'

/**
 * 用 `recent_items` 的时间戳凑一张简版节律。
 *
 * 只给「pulses 缺失但想画点东西」的场合用；**不要**拿它当 cadence 构图的
 * 主视觉（三根线的图没有信息量，那种情况应该走 feature）。
 */
export function pulsesFromTimestamps(
  timestamps: readonly (number | null | undefined)[],
  now: number,
): number[] {
  const MS_PER_DAY = 86_400_000
  return timestamps
    .filter((t): t is number => typeof t === 'number' && t > 0)
    .map((t) => Math.max(0, Math.round((now - t) / MS_PER_DAY)))
    .filter((d) => d <= CADENCE_WINDOW_DAYS)
}
