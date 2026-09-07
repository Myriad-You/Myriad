/**
 * 设置页 hover / focus tooltip：Portal + fixed，避免 overflow 裁切。
 * SettingTitleHelp（ⓘ）与 ToggleSwitch 预告共用这一套，不要再手写一份。
 */

import type { ReactNode, RefObject } from 'react'
import type {
  HoverTooltipCoords,
  HoverTooltipPlacement,
} from './settingHoverTooltip'
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { computeHoverTooltipPosition } from './settingHoverTooltip'
import './SettingTitleHelp.css'

export type SettingHoverTooltipTone = 'default' | 'warning' | 'info'

const HIDE_DELAY_MS = 120

function hasTooltipContent(content: ReactNode): boolean {
  return content != null && content !== false && content !== ''
}

export interface UseSettingHoverTooltipOptions {
  content: ReactNode
  /** false 时 show/hide 为空操作（无预告的开关） */
  enabled?: boolean
  placement?: HoverTooltipPlacement
  tone?: SettingHoverTooltipTone
  tooltipClassName?: string
}

export interface UseSettingHoverTooltipResult<T extends HTMLElement> {
  triggerRef: RefObject<T | null>
  tooltip: ReactNode
  open: boolean
  show: () => void
  hide: () => void
  tooltipId: string
}

export function useSettingHoverTooltip<T extends HTMLElement>(
  options: UseSettingHoverTooltipOptions,
): UseSettingHoverTooltipResult<T> {
  const {
    content,
    enabled = true,
    placement = 'bottom',
    tone = 'default',
    tooltipClassName = '',
  } = options

  const active = enabled && hasTooltipContent(content)
  const tooltipId = useId()
  const triggerRef = useRef<T>(null)
  const tooltipRef = useRef<HTMLDivElement>(null)
  const hideTimerRef = useRef<number>(0)

  const [open, setOpen] = useState(false)
  const [ready, setReady] = useState(false)
  const [coords, setCoords] = useState<HoverTooltipCoords>({
    top: 0,
    left: 0,
    placement,
  })

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current
    const tip = tooltipRef.current
    if (!trigger || !tip) return

    const rect = trigger.getBoundingClientRect()
    const tipW = tip.offsetWidth
    const tipH = tip.offsetHeight
    if (tipW === 0 || tipH === 0) return

    const next = computeHoverTooltipPosition(rect, tipW, tipH, placement, {
      width: window.innerWidth,
      height: window.innerHeight,
    })
    setCoords((prev) =>
      prev.top === next.top &&
      prev.left === next.left &&
      prev.placement === next.placement
        ? prev
        : next,
    )
    setReady(true)
  }, [placement])

  const show = useCallback(() => {
    if (!active) return
    window.clearTimeout(hideTimerRef.current)
    setOpen(true)
  }, [active])

  const hide = useCallback(() => {
    if (!active) return
    window.clearTimeout(hideTimerRef.current)
    hideTimerRef.current = window.setTimeout(() => {
      setOpen(false)
      setReady(false)
    }, HIDE_DELAY_MS)
  }, [active])

  // 不要把 content 放进依赖：调用方常传内联 JSX，引用每次都变，
  // 再叠加 setCoords 会把 layout effect 打进死循环。
  // 文案变高变宽时靠 ResizeObserver 重新量。
  useLayoutEffect(() => {
    if (!open) return
    updatePosition()
    const raf = requestAnimationFrame(updatePosition)
    return () => cancelAnimationFrame(raf)
  }, [open, updatePosition])

  useEffect(() => {
    if (!open) return
    const onReposition = () => updatePosition()
    window.addEventListener('scroll', onReposition, true)
    window.addEventListener('resize', onReposition)
    const tip = tooltipRef.current
    const ro =
      tip && typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(onReposition)
        : null
    if (tip && ro) ro.observe(tip)
    return () => {
      window.removeEventListener('scroll', onReposition, true)
      window.removeEventListener('resize', onReposition)
      ro?.disconnect()
    }
  }, [open, updatePosition])

  useEffect(() => {
    if (!active && open) {
      setOpen(false)
      setReady(false)
    }
  }, [active, open])

  useEffect(
    () => () => {
      window.clearTimeout(hideTimerRef.current)
    },
    [],
  )

  const canPortal = typeof document !== 'undefined'
  const tooltip =
    open && active && canPortal
      ? createPortal(
          <div
            ref={tooltipRef}
            id={tooltipId}
            role="tooltip"
            className={[
              'setting-title-help-tooltip',
              'is-portal',
              ready ? 'is-ready' : '',
              `setting-title-help-tooltip--${tone}`,
              `setting-title-help-tooltip--${coords.placement}`,
              tooltipClassName,
            ]
              .filter(Boolean)
              .join(' ')}
            style={{
              top: coords.top,
              left: coords.left,
            }}
            onMouseEnter={show}
            onMouseLeave={hide}
          >
            {content}
          </div>,
          document.body,
        )
      : null

  return { triggerRef, tooltip, open, show, hide, tooltipId }
}
