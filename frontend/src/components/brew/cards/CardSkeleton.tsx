/**
 * 卡片骨架屏组件
 */

import type { CardSkeletonProps } from '../types'
import React from 'react'
import { SIZE_TO_ROWS } from '../constants'

export const CardSkeleton: React.FC<CardSkeletonProps> = ({
  type,
  count = 1,
  size = 'mini',
}) => {
  const items = Array.from({ length: count }, (_, i) => i)

  if (type === 'source') {
    const rowSpan = SIZE_TO_ROWS[size]

    return (
      <>
        {items.map(i => (
          <div
            key={i}
            className="relative rounded-xl overflow-hidden bg-white/60 dark:bg-neutral-900/60 animate-pulse"
            style={{ gridRow: `span ${rowSpan}` }}
          >
            <div className="absolute inset-0 p-4 flex flex-col">
              {/* 头部 */}
              <div className="flex items-center gap-3">
                <div className="w-9 h-9 rounded-xl bg-gray-200 dark:bg-neutral-700" />
                <div className="flex-1">
                  <div className="h-4 w-24 bg-gray-200 dark:bg-neutral-700 rounded" />
                  <div className="h-3 w-16 bg-gray-200 dark:bg-neutral-700 rounded mt-1.5" />
                </div>
              </div>

              {/* 内容预览 */}
              {size !== 'tiny' && (
                <div className="flex-1 mt-3 p-3 rounded-xl bg-gray-100 dark:bg-neutral-800/50">
                  <div className="h-3 w-full bg-gray-200 dark:bg-neutral-700 rounded" />
                  <div className="h-3 w-3/4 bg-gray-200 dark:bg-neutral-700 rounded mt-2" />
                </div>
              )}
            </div>
          </div>
        ))}
      </>
    )
  }

  // item type
  return (
    <>
      {items.map(i => (
        <div
          key={i}
          className="relative rounded-2xl overflow-hidden bg-white/60 dark:bg-neutral-900/60 animate-pulse"
        >
          <div className="p-6">
            {/* 来源栏 */}
            <div className="flex items-center gap-2 mb-4">
              <div className="h-6 w-20 bg-gray-200 dark:bg-neutral-700 rounded-full" />
              <div className="h-4 w-12 bg-gray-200 dark:bg-neutral-700 rounded" />
            </div>

            {/* 标题 */}
            <div className="h-6 w-full bg-gray-200 dark:bg-neutral-700 rounded" />
            <div className="h-6 w-2/3 bg-gray-200 dark:bg-neutral-700 rounded mt-2" />

            {/* 摘要 */}
            <div className="mt-4 space-y-2">
              <div className="h-4 w-full bg-gray-200 dark:bg-neutral-700 rounded" />
              <div className="h-4 w-full bg-gray-200 dark:bg-neutral-700 rounded" />
              <div className="h-4 w-1/2 bg-gray-200 dark:bg-neutral-700 rounded" />
            </div>
          </div>
        </div>
      ))}
    </>
  )
}

CardSkeleton.displayName = 'CardSkeleton'
