/**
 * 次条行：`[可选缩略图] 单行标题  时间`
 *
 * 字色三档（正常 / dim / dimmer）。4×4 列表从第 3 条起进 dim —— 层次靠颜色
 * 反差，不靠描边分隔。
 */

import type { MouseEvent } from 'react'
import { memo } from 'react'

import { fs, sp, T_META, T_MINOR } from './tokens'

export type MinorDim = 0 | 1 | 2

const DIM_CLASS: Record<MinorDim, string> = {
  0: 'text-gray-600 dark:text-gray-300',
  1: 'text-gray-400 dark:text-gray-500',
  2: 'text-gray-400/70 dark:text-gray-500/70',
}

export interface MinorRowProps {
  title: string
  /** 已格式化的相对时间；空串则不渲染 */
  time?: string
  scale: number
  fontScale: number
  dim?: MinorDim
  /** 点开这一篇。必须 stopPropagation，否则会连带触发整卡点击 */
  onClick?: () => void
}

export const MinorRow = memo(
  ({
    title,
    time,
    scale,
    fontScale,
    dim = 0,
    onClick,
  }: MinorRowProps) => {
    const handleClick = onClick
      ? (e: MouseEvent) => {
          e.stopPropagation()
          onClick()
        }
      : undefined

    return (
      <div
        className={`flex min-w-0 items-center ${onClick ? 'cursor-pointer' : ''}`}
        style={{ gap: sp(6, scale) }}
        onClick={handleClick}
        role={onClick ? 'link' : undefined}
        tabIndex={onClick ? 0 : undefined}
        onKeyDown={
          onClick
            ? (e) => {
                if (e.key === 'Enter') {
                  e.stopPropagation()
                  onClick()
                }
              }
            : undefined
        }
      >
        <span
          className={`min-w-0 flex-1 truncate ${DIM_CLASS[dim]}`}
          style={{ fontSize: fs(T_MINOR, fontScale), lineHeight: 1.35 }}
        >
          {title}
        </span>
        {time ? (
          <span
            className="shrink-0 text-gray-400 dark:text-gray-500"
            style={{ fontSize: fs(T_META, fontScale), lineHeight: 1 }}
          >
            {time}
          </span>
        ) : null}
      </div>
    )
  },
)

MinorRow.displayName = 'MinorRow'
