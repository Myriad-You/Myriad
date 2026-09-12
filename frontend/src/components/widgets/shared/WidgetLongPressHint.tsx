import type { MouseEvent } from 'react'
import { motionShim as motion } from '@lib/motionShim'
import './WidgetLongPressHint.css'

export interface WidgetLongPressHintProps {
  title: string
  visible?: boolean
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
      onClick={(event: MouseEvent<HTMLButtonElement>) => {
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
