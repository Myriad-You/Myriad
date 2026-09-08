/**
 * 磁贴墙的翻页控件。
 *
 * 设计取舍（前一版被否掉的地方）：
 * - **不做悬停才出现的两侧箭头**：发现不了，而且浮在内容上。
 * - **不做 1.5px 高的纯圆点**：视觉上够克制，但点不中 —— 那是装饰不是控件。
 *
 * 现在是一条常驻的胶囊：`‹ ● ▬ ● ›`。前后箭头和圆点在同一个控件里，
 * 一眼能看出「这里能翻页」，每个可点区域都 ≥ 28px，且整条压在墙下方留白里，
 * 不遮任何磁贴。页数多到圆点数不清时（> 7 页）自动换成 `2 / 9` 的读数。
 */

import { LuChevronLeft as ChevronLeft, LuChevronRight as ChevronRight } from '@lib/icons'
import { memo } from 'react'

/** 超过这个页数就不画圆点了，改用数字读数。 */
const DOTS_MAX = 7

export interface BrewPagerProps {
  count: number
  current: number
  onSelect: (page: number) => void
  /** 无障碍标签 */
  labels?: {
    prev?: string
    next?: string
    /** `{n}` 替换成页码 */
    page?: string
  }
}

const BTN =
  'flex h-7 w-7 shrink-0 items-center justify-center rounded-full transition-colors ' +
  'text-gray-500 dark:text-gray-400 ' +
  'hover:bg-black/6 hover:text-gray-700 dark:hover:bg-white/10 dark:hover:text-gray-200 ' +
  'disabled:pointer-events-none disabled:opacity-30'

export const BrewPager = memo(
  ({ count, current, onSelect, labels }: BrewPagerProps) => {
    if (count <= 1) return null

    const pageLabel = (i: number) =>
      (labels?.page ?? '{n}').replace('{n}', String(i + 1))

    return (
      <div className="flex justify-center pt-3">
        <div
          className="glass flex items-center gap-0.5 rounded-full px-1 py-1 shadow-sm"
          role="tablist"
        >
          <button
            type="button"
            className={BTN}
            aria-label={labels?.prev}
            disabled={current === 0}
            onClick={() => onSelect(current - 1)}
          >
            <ChevronLeft className="h-4 w-4" />
          </button>

          {count <= DOTS_MAX ? (
            Array.from({ length: count }, (_, i) => {
              const active = i === current
              return (
                <button
                  key={i}
                  type="button"
                  role="tab"
                  aria-selected={active}
                  aria-label={pageLabel(i)}
                  onClick={() => onSelect(i)}
                  // 28px 的可点区域，里面才是那个小圆点 —— 视觉克制但打得中
                  className="flex h-7 w-7 items-center justify-center rounded-full transition-colors hover:bg-black/6 dark:hover:bg-white/10"
                >
                  <span
                    className={`block rounded-full transition-all duration-300 ${
                      active
                        ? 'h-1.5 w-4 bg-gray-600 dark:bg-gray-200'
                        : 'h-1.5 w-1.5 bg-gray-300 dark:bg-neutral-600'
                    }`}
                  />
                </button>
              )
            })
          ) : (
            <span className="px-2 text-[11px] tabular-nums text-gray-500 dark:text-gray-400">
              {current + 1} / {count}
            </span>
          )}

          <button
            type="button"
            className={BTN}
            aria-label={labels?.next}
            disabled={current === count - 1}
            onClick={() => onSelect(current + 1)}
          >
            <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
    )
  },
)

BrewPager.displayName = 'BrewPager'
