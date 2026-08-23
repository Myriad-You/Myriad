/**
 * 磁贴墙的分页指示器。
 *
 * 控制面板没有这东西（它固定 3 页、用户知道），Brew 的页数由源数量决定，
 * 不给指示器就等于藏起一半内容 —— 游客也必须看得见。
 */

import { memo } from 'react'

export interface BrewPageDotsProps {
  count: number
  current: number
  onSelect: (page: number) => void
  /** 无障碍标签模板，`{n}` 替换成页码 */
  labelTemplate?: string
}

export const BrewPageDots = memo(
  ({ count, current, onSelect, labelTemplate }: BrewPageDotsProps) => {
    if (count <= 1) return null

    return (
      <div
        className="flex items-center justify-center gap-1.5 pt-4"
        role="tablist"
      >
        {Array.from({ length: count }, (_, i) => {
          const active = i === current
          return (
            <button
              key={i}
              type="button"
              role="tab"
              aria-selected={active}
              aria-label={(labelTemplate ?? '{n}').replace('{n}', String(i + 1))}
              onClick={() => onSelect(i)}
              className={`h-1.5 rounded-full transition-all duration-300 ${
                active
                  ? 'w-5 bg-gray-500 dark:bg-gray-300'
                  : 'w-1.5 bg-gray-300 hover:bg-gray-400 dark:bg-neutral-600 dark:hover:bg-neutral-500'
              }`}
            />
          )
        })}
      </div>
    )
  },
)

BrewPageDots.displayName = 'BrewPageDots'
