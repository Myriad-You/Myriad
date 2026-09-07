/**
 * 编辑模式下「长按进入单独卡片设置」的通用齿轮提示。
 *
 * 统一左上角位置与样式。可点开设置气泡；卡片长按仍可用。
 */

import type { MouseEvent } from 'react'
import { motionShim as motion } from '@lib/motionShim'
import './WidgetLongPressHint.css'

export interface WidgetLongPressHintProps {
  /** 悬停 title（各小组件 i18n：longPressHint / longPressToEdit） */
  title: string
  /**
   * 是否显示。默认 true。
   * 调用方可写 `visible={isEditMode}`，或外层条件渲染。
   */
  visible?: boolean
  /** 追加到根节点的 className（少用；默认布局勿轻易覆盖） */
  className?: string
  onClick?: () => void
}

const BASE_CLASS = 'widget-longpress-hint'

export function WidgetLongPressHint({
  title,
  visible = true,
  className,
  onClick,
}: WidgetLongPressHintProps) {
  if (!visible) return null

  const handleMouseDown = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation()
    event.preventDefault()
  }

  return (
    <motion.button
      type="button"
      className={className ? `${BASE_CLASS} ${className}` : BASE_CLASS}
      initial={{ opacity: 0, scale: 0.72, x: -4, y: -4 }}
      animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
      transition={{ type: 'spring', stiffness: 380, damping: 22 }}
      title={title}
      aria-label={title}
      onMouseDown={handleMouseDown}
      onClick={(event) => {
        event.stopPropagation()
        onClick?.()
      }}
    >
      <svg
        className="widget-longpress-hint__icon"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
        />
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
        />
      </svg>
    </motion.button>
  )
}
