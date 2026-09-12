import type { CSSProperties, ReactNode } from 'react'
import { LuX } from '@lib/icons'
import { createContext, useContext, useEffect, useId, useLayoutEffect, useReducer, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { useAnimationLevel } from '../../../hooks/useAnimationLevel'
import { usePerformanceProfile } from '../../../hooks/usePerformanceProfile'
import {
  initialPanelState,
  isPanelMorphing,
  isPanelOpen,
  panelReducer,
  resolvePanelMotion,
  settleTimeoutMs,
} from '../../ControlPanel/panelTransition'

const DisplayControl = createContext<ReactNode>(null)
export const useManagementDisplay = () => useContext(DisplayControl)

const AccessorySlot = createContext<HTMLDivElement | null>(null)
export const useManagementAccessory = () => useContext(AccessorySlot)

export function BrewManagement({ children, embedded, active, displayControl }: {
  children: ReactNode
  displayControl?: ReactNode
  embedded: boolean
  active?: boolean
}) {
  const { t } = useI18n()
  const [accessory, setAccessory] = useState<HTMLDivElement | null>(null)
  const [panel, dispatch] = useReducer(panelReducer, initialPanelState)
  const root = useRef<HTMLDivElement>(null)
  const content = useRef<HTMLDivElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  const [width, setWidth] = useState(48)
  const id = useId()
  const anim = useAnimationLevel()
  const perf = usePerformanceProfile()
  const profile = resolvePanelMotion({ level: anim.level, reduceMotion: perf.reduceMotion })
  const [motion, setMotion] = useState(profile)
  const expanded = isPanelOpen(panel)

  useEffect(() => {
    if (!isPanelMorphing(panel)) setMotion(profile)
  }, [panel.phase, profile.morphMs, profile.spatial, profile.tabMs])

  useLayoutEffect(() => {
    const el = content.current
    if (!el) return
    const measure = () => setWidth(Math.ceil(el.scrollWidth))
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (active) dispatch({ type: 'open' })
  }, [active])

  useEffect(() => {
    if (!isPanelMorphing(panel)) return
    const generation = panel.generation
    const timer = window.setTimeout(dispatch, settleTimeoutMs(motion), { type: 'settle', generation })
    return () => window.clearTimeout(timer)
  }, [panel.phase, panel.generation, motion])

  useEffect(() => {
    if (!expanded) return
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) dispatch({ type: 'close' })
    }
    document.addEventListener('pointerdown', dismiss)
    return () => document.removeEventListener('pointerdown', dismiss)
  }, [expanded])

  useEffect(() => {
    if (panel.phase === 'expanded' && document.activeElement === trigger.current) {
      closeButton.current?.focus({ preventScroll: true })
    }
  }, [panel.phase])

  const close = () => {
    dispatch({ type: 'close' })
    trigger.current?.focus()
  }

  return (
    <DisplayControl.Provider value={displayControl}>
    <AccessorySlot.Provider value={accessory}>
    <div className={`brew-management-tools${embedded ? '' : ' brew-management--page'}`}>
    <div
      ref={root}
      className="brew-skin brew-management"
      data-open={expanded}
      data-phase={panel.phase}
      data-spatial={motion.spatial}
      style={{
        '--brew-management-ms': `${motion.morphMs}ms`,
        '--brew-management-width': `${width}px`,
      } as CSSProperties}
      onTransitionEnd={(event) => {
        if (event.target === event.currentTarget && event.propertyName === 'width') {
          dispatch({ type: 'settle', generation: panel.generation })
        }
      }}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && expanded) {
          event.stopPropagation()
          close()
        }
      }}
    >
      <button
        ref={trigger}
        type="button"
        className="brew-management__trigger"
        aria-label={t.brew.expandMenu}
        title={t.brew.expandMenu}
        aria-expanded={expanded}
        aria-controls={id}
        tabIndex={expanded ? -1 : 0}
        onClick={() => {
          dispatch({ type: 'open' })
        }}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden><path d="M4 6h16M4 12h16M4 18h16" /></svg>
      </button>
      <div
        ref={content}
        id={id}
        className="brew-management__body"
        inert={!expanded}
        aria-hidden={!expanded}
      >
        <button className="brew-management__close" ref={closeButton} type="button" aria-label={t.brew.close} onClick={close}>
          <LuX aria-hidden />
        </button>
        <div className="brew-management__actions">{children}</div>
      </div>
    </div>
    <div className="brew-management-accessory" ref={setAccessory} />
    </div>
    </AccessorySlot.Provider>
    </DisplayControl.Provider>
  )
}
