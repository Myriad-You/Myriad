/**
 * 编辑模式下「长按进入单独卡片设置」的通用齿轮提示。
 *
 * 统一右下角位置与样式，避免各小组件复制同一套 motion + SVG。
 * 仅视觉提示（pointer-events-none），实际设置由卡片长按手势打开。
 */

import { motionShim as motion } from '@lib/motionShim'

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
}

const BASE_CLASS =
  'absolute bottom-1.5 right-1.5 z-30 w-5 h-5 rounded-md flex items-center justify-center bg-black/15 dark:bg-white/15 backdrop-blur-sm pointer-events-none'

export function WidgetLongPressHint({
  title,
  visible = true,
  className,
}: WidgetLongPressHintProps) {
  if (!visible) return null

  return (
    <motion.div
      className={className ? `${BASE_CLASS} ${className}` : BASE_CLASS}
      initial={{ opacity: 0, scale: 0.8 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ type: 'spring', stiffness: 400, damping: 20 }}
      title={title}
      aria-hidden
    >
      <svg
        className="w-3 h-3 text-gray-700 dark:text-gray-200"
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
    </motion.div>
  )
}
