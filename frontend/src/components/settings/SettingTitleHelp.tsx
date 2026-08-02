/**
 * 标题旁短说明 ⓘ：默认 hover / focus 以 tooltip 显示。
 * Tooltip 通过 Portal 挂到 document.body + fixed 定位，避免被 overflow 裁切。
 *
 * 仅用于 detail / description 短文案。「显示说明」开启后由父组件改为标题下常显。
 * 结构化长指南请用 SettingTitleGuideEntry（点击展开），不要塞进本组件。
 */

import type { ReactNode } from 'react'
import { LuInfo } from '@lib/icons'
import React, {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../contexts/I18nContext'
import './SettingTitleHelp.css'

export type SettingTitleHelpTone = 'default' | 'warning' | 'info'

export interface SettingTitleHelpProps {
  /** 详细说明内容（支持富文本 / 链接） */
  children: ReactNode
  /** 触发器无障碍名 */
  ariaLabel?: string
  /** 视觉语气：warning 用于阻断性提示 */
  tone?: SettingTitleHelpTone
  /** 首选方向；空间不足时自动翻转 */
  placement?: 'top' | 'bottom'
  className?: string
}

const VIEWPORT_PAD = 8
const GAP = 6

interface Coords {
  top: number
  left: number
  placement: 'top' | 'bottom'
}

function computePosition(
  trigger: DOMRect,
  tipW: number,
  tipH: number,
  preferred: 'top' | 'bottom',
): Coords {
  const vw = window.innerWidth
  const vh = window.innerHeight

  let placement: 'top' | 'bottom' = preferred
  const spaceBelow = vh - trigger.bottom - VIEWPORT_PAD
  const spaceAbove = trigger.top - VIEWPORT_PAD

  if (
    placement === 'bottom' &&
    tipH + GAP > spaceBelow &&
    spaceAbove > spaceBelow
  ) {
    placement = 'top'
  } else if (
    placement === 'top' &&
    tipH + GAP > spaceAbove &&
    spaceBelow >= spaceAbove
  ) {
    placement = 'bottom'
  }

  let left = trigger.left + trigger.width / 2 - tipW / 2
  left = Math.max(VIEWPORT_PAD, Math.min(left, vw - tipW - VIEWPORT_PAD))

  const top =
    placement === 'bottom'
      ? trigger.bottom + GAP
      : trigger.top - GAP - tipH

  const clampedTop = Math.max(
    VIEWPORT_PAD,
    Math.min(top, vh - tipH - VIEWPORT_PAD),
  )

  return { top: clampedTop, left, placement }
}

export const SettingTitleHelp: React.FC<SettingTitleHelpProps> = ({
  children,
  ariaLabel: ariaLabelProp,
  tone = 'default',
  placement = 'bottom',
  className = '',
}) => {
  const { t } = useI18n()
  const ariaLabel = ariaLabelProp ?? t.config.detailHelpAria
  const tooltipId = useId()
  const triggerRef = useRef<HTMLButtonElement>(null)
  const tooltipRef = useRef<HTMLDivElement>(null)
  const hideTimerRef = useRef<number>(0)

  const [open, setOpen] = useState(false)
  const [ready, setReady] = useState(false)
  const [coords, setCoords] = useState<Coords>({
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

    setCoords(computePosition(rect, tipW, tipH, placement))
    setReady(true)
  }, [placement])

  const show = useCallback(() => {
    window.clearTimeout(hideTimerRef.current)
    setOpen(true)
  }, [])

  const hide = useCallback(() => {
    window.clearTimeout(hideTimerRef.current)
    hideTimerRef.current = window.setTimeout(() => {
      setOpen(false)
      setReady(false)
    }, 120)
  }, [])

  useLayoutEffect(() => {
    if (!open) return
    updatePosition()
    const raf = requestAnimationFrame(updatePosition)
    return () => cancelAnimationFrame(raf)
  }, [open, children, updatePosition])

  useEffect(() => {
    if (!open) return
    const onReposition = () => updatePosition()
    window.addEventListener('scroll', onReposition, true)
    window.addEventListener('resize', onReposition)
    return () => {
      window.removeEventListener('scroll', onReposition, true)
      window.removeEventListener('resize', onReposition)
    }
  }, [open, updatePosition])

  useEffect(
    () => () => {
      window.clearTimeout(hideTimerRef.current)
    },
    [],
  )

  if (children == null || children === false || children === '') return null

  const canPortal = typeof document !== 'undefined'

  const tooltip =
    open && canPortal
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
            {children}
          </div>,
          document.body,
        )
      : null

  return (
    <span
      className={[
        'setting-title-help',
        `setting-title-help--${tone}`,
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <button
        ref={triggerRef}
        type="button"
        className="setting-title-help-trigger"
        aria-label={ariaLabel}
        aria-describedby={open ? tooltipId : undefined}
        aria-expanded={open}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        <LuInfo className="setting-title-help-icon" aria-hidden />
      </button>
      {tooltip}
    </span>
  )
}

SettingTitleHelp.displayName = 'SettingTitleHelp'

export default SettingTitleHelp
