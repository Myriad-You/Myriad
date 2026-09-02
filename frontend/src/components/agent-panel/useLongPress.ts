/**
 * useLongPress - 长按检测 hook
 *
 * 全局长按手势：
 * - 检测 mouse/touch 长按事件
 * - 超过阈值后触发回调；没有回调时只走指示动效与震动
 * - 移动超过 10px 自动取消
 * - 只认主键（左键 / 触控）；右键 / 中键不触发，避免抢系统菜单
 * - 自动排除交互元素
 */

import { useCallback, useEffect, useRef, useState } from 'react'

interface LongPressIndicator {
  x: number
  y: number
  active: boolean
}

/**
 * 长按不该抢走的地方：面板自己、可交互控件、站点 chrome。
 *
 * 这里没有 Tapp 窗口。窗口内容跑在 iframe 里，mousedown 根本不冒泡到宿主文档，
 * 长按天然够不着；窗口 chrome（标题栏、边框）则该和页面其他空白处一样能唤起，
 * 它自己的按钮和输入框由下面的通用控件选择器兜住。
 */
const EXCLUDED_SELECTORS =
  '.agent-panel-overlay-anchor, input, textarea, button, a, [contenteditable], .global-control-bar, .control-panel-overlay'

/** 按住多久算长按 (ms)。 */
export const LONG_PRESS_DURATION = 500

export function useLongPress(
  duration: number,
  /** 手势达成后的落点。留空表示手势仍然识别，但不接任何动作。 */
  onTrigger: (() => void) | undefined,
  enabled: boolean,
) {
  const timerRef = useRef<NodeJS.Timeout | null>(null)
  const startRef = useRef<{ x: number; y: number } | null>(null)
  const isPressing = useRef(false)
  const [indicator, setIndicator] = useState<LongPressIndicator>({
    x: 0,
    y: 0,
    active: false,
  })

  const dropDragRef = useRef<() => void>(() => {})

  const cancel = useCallback(() => {
    dropDragRef.current()
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
    isPressing.current = false
    startRef.current = null
    setIndicator((prev) => ({ ...prev, active: false }))
  }, [])

  const start = useCallback(
    (e: MouseEvent | TouchEvent) => {
      if (!enabled) return
      // MouseEvent only: primary button (0). Right/middle must not open agent.
      if ('button' in e && e.button !== 0) return
      const target = e.target as HTMLElement
      if (target.closest(EXCLUDED_SELECTORS)) return

      const point = 'touches' in e ? e.touches[0] : e
      startRef.current = { x: point.clientX, y: point.clientY }
      isPressing.current = true
      setIndicator({ x: point.clientX, y: point.clientY, active: true })

      timerRef.current = setTimeout(() => {
        if (isPressing.current) {
          setIndicator((prev) => ({ ...prev, active: false }))
          onTrigger?.()
          if (navigator.vibrate) navigator.vibrate(50)
        }
      }, duration)
    },
    [enabled, duration, onTrigger],
  )

  const checkMovement = useCallback(
    (e: MouseEvent | TouchEvent) => {
      if (!startRef.current || !isPressing.current) return
      const point = 'touches' in e ? e.touches[0] : e
      const dx = Math.abs(point.clientX - startRef.current.x)
      const dy = Math.abs(point.clientY - startRef.current.y)
      if (dx > 10 || dy > 10) cancel()
    },
    [cancel],
  )

  useEffect(() => {
    if (!enabled) {
      cancel()
      return undefined
    }

    let dragging = false
    const handleMove = (e: MouseEvent | TouchEvent) => checkMovement(e)
    const handleUp = () => cancel()
    const dropDrag = () => {
      if (!dragging) return
      dragging = false
      document.removeEventListener('mouseup', handleUp)
      document.removeEventListener('touchend', handleUp)
      document.removeEventListener('touchcancel', handleUp)
      document.removeEventListener('mousemove', handleMove)
      document.removeEventListener('touchmove', handleMove)
      document.removeEventListener('contextmenu', handleUp)
    }
    const takeDrag = () => {
      if (dragging) return
      dragging = true
      document.addEventListener('mouseup', handleUp)
      document.addEventListener('touchend', handleUp)
      document.addEventListener('touchcancel', handleUp)
      document.addEventListener('mousemove', handleMove)
      document.addEventListener('touchmove', handleMove, { passive: true })
      document.addEventListener('contextmenu', handleUp)
    }
    dropDragRef.current = dropDrag

    const handleMouseDown = (e: MouseEvent) => {
      start(e)
      if (isPressing.current) takeDrag()
    }
    const handleTouchStart = (e: TouchEvent) => {
      start(e)
      if (isPressing.current) takeDrag()
    }

    document.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('touchstart', handleTouchStart, {
      passive: true,
    })

    return () => {
      document.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('touchstart', handleTouchStart)
      cancel()
      dropDragRef.current = () => {}
    }
  }, [enabled, start, cancel, checkMovement])

  return { indicator }
}
