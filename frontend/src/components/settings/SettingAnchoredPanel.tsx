import type { ReactNode } from 'react'
import React, {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import './SettingTitleHelp.css'
import './SettingAnchoredPanel.css'

export type SettingAnchoredPanelPlacement = 'top' | 'bottom'

export interface SettingAnchoredPanelTriggerApi {
  open: boolean
  disabled: boolean
  toggle: () => void
  openPanel: () => void
  closePanel: () => void
}

export interface SettingAnchoredPanelProps {
  trigger:
    | ReactNode
    | ((api: SettingAnchoredPanelTriggerApi) => ReactNode)
  children: ReactNode
  open?: boolean
  onOpenChange?: (open: boolean) => void
  defaultOpen?: boolean
  /** true: no outside/Esc/toggle close */
  preventClose?: boolean
  disabled?: boolean
  placement?: SettingAnchoredPanelPlacement
  align?: 'start' | 'center'
  ariaLabel?: string
  className?: string
  panelClassName?: string
}

const VIEWPORT_PAD = 8
const GAP = 8

interface Coords {
  top: number
  left: number
  placement: SettingAnchoredPanelPlacement
}

function computePosition(
  trigger: DOMRect,
  tipW: number,
  tipH: number,
  preferred: SettingAnchoredPanelPlacement,
  align: 'start' | 'center',
): Coords {
  const vw = window.innerWidth
  const vh = window.innerHeight

  let placement: SettingAnchoredPanelPlacement = preferred
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

  let left =
    align === 'center'
      ? trigger.left + trigger.width / 2 - tipW / 2
      : trigger.left
  left = Math.max(VIEWPORT_PAD, Math.min(left, vw - tipW - VIEWPORT_PAD))

  let top =
    placement === 'bottom'
      ? trigger.bottom + GAP
      : trigger.top - GAP - tipH
  top = Math.max(VIEWPORT_PAD, Math.min(top, vh - tipH - VIEWPORT_PAD))

  return { top, left, placement }
}

export const SettingAnchoredPanel: React.FC<SettingAnchoredPanelProps> = ({
  trigger,
  children,
  open: openControlled,
  onOpenChange,
  defaultOpen = false,
  preventClose = false,
  disabled = false,
  placement = 'bottom',
  align = 'start',
  ariaLabel,
  className = '',
  panelClassName = '',
}) => {
  const isControlled = openControlled !== undefined
  const [uncontrolledOpen, setUncontrolledOpen] = useState(defaultOpen)
  const open = isControlled ? Boolean(openControlled) : uncontrolledOpen

  const setOpen = useCallback(
    (next: boolean, force = false) => {
      if (disabled && next) return
      if (!force && preventClose && !next) return
      if (!isControlled) setUncontrolledOpen(next)
      onOpenChange?.(next)
    },
    [disabled, isControlled, onOpenChange, preventClose],
  )

  const toggle = useCallback(() => {
    setOpen(!open)
  }, [open, setOpen])

  const openPanel = useCallback(() => setOpen(true), [setOpen])
  const closePanel = useCallback(() => setOpen(false, true), [setOpen])

  const triggerWrapRef = useRef<HTMLSpanElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const panelId = useId()

  const [ready, setReady] = useState(false)
  const [coords, setCoords] = useState<Coords>({
    top: 0,
    left: 0,
    placement,
  })

  const updatePosition = useCallback(() => {
    const triggerEl = triggerWrapRef.current
    const panel = panelRef.current
    if (!triggerEl || !panel) return
    const rect = triggerEl.getBoundingClientRect()
    const tipW = panel.offsetWidth
    const tipH = panel.offsetHeight
    if (tipW === 0 || tipH === 0) return
    setCoords(computePosition(rect, tipW, tipH, placement, align))
    setReady(true)
  }, [align, placement])

  useLayoutEffect(() => {
    if (!open) {
      setReady(false)
      return
    }
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

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    const onPointer = (e: MouseEvent) => {
      const node = e.target as Node | null
      if (panelRef.current?.contains(node)) return
      if (triggerWrapRef.current?.contains(node)) return
      setOpen(false)
    }
    const tid = window.setTimeout(() => {
      window.addEventListener('mousedown', onPointer)
    }, 0)
    window.addEventListener('keydown', onKey)
    return () => {
      window.clearTimeout(tid)
      window.removeEventListener('mousedown', onPointer)
      window.removeEventListener('keydown', onKey)
    }
  }, [open, setOpen])

  const api: SettingAnchoredPanelTriggerApi = {
    open,
    disabled,
    toggle,
    openPanel,
    closePanel,
  }

  const triggerNode =
    typeof trigger === 'function' ? trigger(api) : trigger

  const canPortal = typeof document !== 'undefined'
  const panel =
    open && canPortal
      ? createPortal(
          <div
            ref={panelRef}
            id={panelId}
            role="dialog"
            aria-modal="false"
            aria-label={ariaLabel}
            className={[
              'setting-title-help-tooltip',
              'is-portal',
              'setting-anchored-panel-surface',
              ready ? 'is-ready' : '',
              `setting-title-help-tooltip--${coords.placement}`,
              panelClassName,
            ]
              .filter(Boolean)
              .join(' ')}
            style={{ top: coords.top, left: coords.left }}
          >
            {children}
          </div>,
          document.body,
        )
      : null

  return (
    <span
      ref={triggerWrapRef}
      className={['setting-anchored-panel', className].filter(Boolean).join(' ')}
      data-open={open ? 'true' : undefined}
    >
      {triggerNode}
      {panel}
    </span>
  )
}

SettingAnchoredPanel.displayName = 'SettingAnchoredPanel'

export default SettingAnchoredPanel
