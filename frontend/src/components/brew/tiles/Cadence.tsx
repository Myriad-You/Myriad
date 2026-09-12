/** x 轴 1-sqrt 压缩，右端是今天。高度缺 reading_time 时用天数伪随机。除横轴外不画线。 */

import { memo, useMemo } from 'react'

/** 与后端 PULSE_WINDOW_DAYS 一致。 */
export const CADENCE_WINDOW_DAYS = 730

const BAR_WIDTH = 1.5

/** 同一天数永远同一高度。 */
function pseudoHeight(days: number): number {
  const v = Math.sin(days * 12.9898 + 78.233) * 43758.5453
  return 0.35 + (v - Math.floor(v)) * 0.65
}

export interface CadenceProps {
  pulses: readonly number[]
  color: string
  height: number
  readingTimes?: readonly (number | null | undefined)[]
  className?: string
}

export const Cadence = memo(
  ({ pulses, color, height, readingTimes, className }: CadenceProps) => {
    const bars = useMemo(() => {
      return pulses
        .filter((d) => Number.isFinite(d) && d >= 0 && d <= CADENCE_WINDOW_DAYS)
        .map((days, i) => {
          const x = (1 - Math.sqrt(days / CADENCE_WINDOW_DAYS)) * 100
          const minutes = readingTimes?.[i]
          const ratio =
            typeof minutes === 'number' && minutes > 0
              ? Math.min(1, 0.35 + minutes / 18)
              : pseudoHeight(days)
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
        {/* 横轴是唯一允许的线。 */}
        <span
          className="absolute inset-x-0 bottom-0 h-px"
          style={{ background: color, opacity: 0.18 }}
        />
      </div>
    )
  },
)

Cadence.displayName = 'Cadence'

/** 不要拿简版节律当 cadence 主视觉。 */
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
