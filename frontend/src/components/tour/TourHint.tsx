import {
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type AnimationEvent,
} from 'react'
import { useLocation } from 'react-router-dom'
import { LuArrowRight, LuX } from '@lib/icons'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import {
  prefersReducedMotion,
  SETTINGS_DURATION_MS,
} from '../settings/motion'
import { getCurrentMetadata } from '../../utils/siteMetadata'
import {
  getTourDoneSnapshot,
  isTourDone,
  markTourDone,
  subscribeTourDone,
} from './tourDone'
import {
  getTourSnapshot,
  startTour,
  stopTour,
  subscribeTour,
} from './tourEngine'
import {
  fillTourHint,
  getConfigTourSurface,
  getLibraryTourSurfaceSnapshot,
  isTourAnchorMeasurable,
  LIBRARY_FILTER_EXPAND_WAIT_MS,
  waitForTourAnchor,
  isHomeEditSurface,
  pageNameForPath,
  readTourSurface,
  refreshConfigTourSurface,
  revealTourAnchor,
  shouldAbortLibraryTour,
  subscribeConfigTourSurface,
  subscribeHomeEditSurface,
  subscribeLibraryCanvasTourSurface,
} from './tourLogic'
import {
  shouldAutoHideTourHint,
  TOUR_HINT_AUTO_HIDE_MS,
} from './tourHintLogic'
import { pickRegisteredTour } from './tourRegistry'
import './TourHint.css'

function skipTourHintMotion(): boolean {
  if (prefersReducedMotion()) return true
  if (typeof document === 'undefined') return false
  return document.documentElement.dataset.perfMode === 'exlight'
}

function getTourActive(): boolean {
  return getTourSnapshot().active
}

export function TourHint() {
  const { t } = useI18n()
  const location = useLocation()
  const { isAdmin, hasChecked } = useAuth()
  const meta = getCurrentMetadata()
  const siteName = meta.site_title.trim() || 'Myriad'
  const siteLogo = meta.site_favicon.trim() || '/favicon.webp'
  const editingHome = useSyncExternalStore(
    subscribeHomeEditSurface,
    isHomeEditSurface,
    isHomeEditSurface,
  )
  useSyncExternalStore(
    subscribeConfigTourSurface,
    getConfigTourSurface,
    getConfigTourSurface,
  )
  const librarySurface = useSyncExternalStore(
    subscribeLibraryCanvasTourSurface,
    getLibraryTourSurfaceSnapshot,
    getLibraryTourSurfaceSnapshot,
  )
  const surface = readTourSurface(editingHome, location.pathname)
  const pageName =
    surface === 'persona' || surface === 'ai-persona'
      ? t.widgets.agentPersona
      : pageNameForPath(
          location.pathname,
          t.nav,
          t.common.editMode,
          editingHome,
        )
  const tourActive = useSyncExternalStore(
    subscribeTour,
    getTourActive,
    getTourActive,
  )
  useSyncExternalStore(subscribeTourDone, getTourDoneSnapshot, getTourDoneSnapshot)
  const [leaving, setLeaving] = useState<'dismiss' | 'start' | null>(null)
  const [snoozedId, setSnoozedId] = useState<string | null>(null)
  const leaveTimer = useRef(0)
  const autoHideTimer = useRef(0)
  const leaveAction = useRef<(() => void) | null>(null)
  const leavingRef = useRef(leaving)
  leavingRef.current = leaving

  const def = pickRegisteredTour(location.pathname, isAdmin, surface)

  const finishLeaveAction = () => {
    const action = leaveAction.current
    leaveAction.current = null
    window.clearTimeout(leaveTimer.current)
    action?.()
  }

  const finishLeave = (kind: 'dismiss' | 'start', action: () => void) => {
    if (leavingRef.current) return
    window.clearTimeout(autoHideTimer.current)
    if (skipTourHintMotion()) {
      action()
      return
    }
    leaveAction.current = action
    setLeaving(kind)
    window.clearTimeout(leaveTimer.current)
    leaveTimer.current = window.setTimeout(
      finishLeaveAction,
      SETTINGS_DURATION_MS.slow + 80,
    )
  }

  useEffect(() => {
    refreshConfigTourSurface()
  }, [location.pathname, location.search])

  useEffect(() => {
    setLeaving(null)
    setSnoozedId(null)
    leaveAction.current = null
    return () => {
      window.clearTimeout(leaveTimer.current)
      window.clearTimeout(autoHideTimer.current)
    }
  }, [def?.id])

  useEffect(() => {
    window.clearTimeout(autoHideTimer.current)
    if (!shouldAutoHideTourHint()) return
    if (!def?.id || !hasChecked || tourActive || leaving) return
    if (isTourDone(def.id) || snoozedId === def.id) return
    autoHideTimer.current = window.setTimeout(() => {
      finishLeave('dismiss', () => setSnoozedId(def.id))
    }, TOUR_HINT_AUTO_HIDE_MS)
    return () => window.clearTimeout(autoHideTimer.current)
  }, [def?.id, hasChecked, tourActive, snoozedId, leaving])

  useEffect(() => {
    const snapshot = getTourSnapshot()
    if (!snapshot.active || !snapshot.tourId) return
    if (surface === 'none') {
      stopTour('abort')
      return
    }
    if (snapshot.tourId === 'config-owner' && surface !== 'browse') {
      stopTour('abort')
      return
    }
    if (snapshot.tourId === 'config-ai-persona-owner' && surface !== 'ai-persona') {
      stopTour('abort')
      return
    }
    if (snapshot.tourId === 'config-persona-owner' && surface !== 'persona') {
      stopTour('abort')
      return
    }
    if (shouldAbortLibraryTour(snapshot.tourId, librarySurface)) {
      stopTour('abort')
    }
  }, [librarySurface, surface])

  if (!hasChecked || tourActive) return null
  if (librarySurface === 'pending') return null
  if (!def) return null
  if (isTourDone(def.id)) return null
  if (snoozedId === def.id && !leaving) return null

  const handleLeaveEnd = (event: AnimationEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return
    if (!event.animationName.startsWith('tour-hint-leave')) return
    if (!leaving) return
    finishLeaveAction()
  }

  return (
    <aside
      className={`tour-hint${leaving ? ` is-leaving is-leaving--${leaving}` : ''}`}
      aria-label={t.tour.hint}
      onAnimationEnd={handleLeaveEnd}
    >
      <div className="tour-hint__face">
        <button
          type="button"
          className="tour-hint__close"
          aria-label={t.common.close}
          onClick={() => finishLeave('dismiss', () => markTourDone(def.id))}
        >
          <LuX aria-hidden />
        </button>
        <img
          className="tour-hint__logo"
          src={siteLogo}
          alt=""
          width={44}
          height={44}
        />
        <div className="tour-hint__main">
          <div className="tour-hint__copy">
            <h2 className="tour-hint__title">
              {fillTourHint(t.tour.hintWelcome, 'site', siteName)}
            </h2>
            <p className="tour-hint__body">
              {fillTourHint(t.tour.hintTitle, 'page', pageName) || t.tour.hintBody}
            </p>
          </div>
          <button
            type="button"
            className="tour-hint__go"
            onClick={() => {
              if (def.route === '/library') {
                window.dispatchEvent(
                  new CustomEvent('nav-expand-secondary', {
                    detail: { path: '/library' },
                  }),
                )
              }
              const first = def.steps.find((step) =>
                isTourAnchorMeasurable(step.anchor),
              )
              if (first) revealTourAnchor(first.anchor, first.id)
              finishLeave('start', () => {
                const begin = () => {
                  if (!startTour(def)) setLeaving(null)
                }
                if (def.route !== '/library') {
                  begin()
                  return
                }
                void waitForTourAnchor(
                  'library-filters',
                  LIBRARY_FILTER_EXPAND_WAIT_MS,
                ).then(begin)
              })
            }}
          >
            <span>{t.tour.begin || t.common.go}</span>
            <LuArrowRight aria-hidden />
          </button>
        </div>
      </div>
    </aside>
  )
}
