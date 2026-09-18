/** 层次靠颜色反差，不靠描边。 */

import type { MouseEvent } from 'react'
import { memo } from 'react'

import { fs, sp, T_META, T_MINOR } from './tokens'

type MinorDim = 0 | 1 | 2

const DIM_CLASS: Record<MinorDim, string> = {
  0: 'text-gray-600 dark:text-gray-300',
  1: 'text-gray-400 dark:text-gray-500',
  2: 'text-gray-400/70 dark:text-gray-500/70',
}

interface MinorRowProps {
  title: string
  time?: string
  scale: number
  fontScale: number
  dim?: MinorDim
  titleSize?: number
  metaSize?: number
  /** 必须 stopPropagation，否则会点开整卡。 */
  onClick?: () => void
}

export const MinorRow = memo(
  ({
    title,
    time,
    scale,
    fontScale,
    dim = 0,
    titleSize = T_MINOR,
    metaSize = T_META,
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
          style={{ fontSize: fs(titleSize, fontScale), lineHeight: 1.35 }}
        >
          {title}
        </span>
        {time ? (
          <span
            className="shrink-0 text-gray-400 dark:text-gray-500"
            style={{ fontSize: fs(metaSize, fontScale), lineHeight: 1 }}
          >
            {time}
          </span>
        ) : null}
      </div>
    )
  },
)

MinorRow.displayName = 'MinorRow'
