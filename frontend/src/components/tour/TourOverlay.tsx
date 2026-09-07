import type { TourPlacement } from './tourLogic'
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { createPortal } from 'react-dom'
import { useLocation } from 'react-router-dom'
import { LuArrowRight, LuCheck, LuChevronLeft } from '@lib/icons'
import { useI18n } from '../../contexts/I18nContext'
import {
  getTourSnapshot,
  nextTourStep,
  previousTourStep,
  recoverTourStep,
  stopTour,
  subscribeTour,
} from './tourEngine'
import {
  computeTourCardPosition,
  dockTourCard,
  holePadForTourAnchor,
  holeRadiusFor,
  inflateRect,
  isDegenerateBox,
  isPredictedTourAnchor,
  predictedControlIslandHoleRadius,
  predictedControlIslandTourBox,
  predictedControlPanelHoleRadius,
  predictedControlPanelTourBox,
  predictedLibraryDockTourBox,
  queryTourAnchor,
  readControlPanelContentHeight,
  readTourBox,
  resolveTourMeasureNode,
  rootFontSizePx,
  sameTourCardPos,
  sameTourHole,
  TOUR_VIEWPORT_PAD,
  tourHoleSync,
  tourMeasureWatchesHost,
  tourMeasureWatchesScroll,
} from './tourLogic'
import './TourOverlay.css'

const CARD_FALLBACK = { w: 296, h: 176 }
const CARD_POS_FALLBACK = { top: 24, left: 24, placement: 'bottom' as const }
const EMPTY_HOLE = { top: 0, left: 0, width: 0, height: 0, radius: 16 }
const PULSE_MS = 520
const SKIP_HOLD_MS = 500
const SKIP_HOLD_SLOP = 10

interface StepCopy {
  title: string
  body: string
}

function stepCopy(
  steps: Record<string, StepCopy>,
  id: string,
): StepCopy {
  return steps[id] ?? { title: id, body: '' }
}

function readRadius(node: HTMLElement): number {
  const raw = Number.parseFloat(getComputedStyle(node).borderTopLeftRadius)
  return Number.isFinite(raw) ? raw : 16
}

export function TourOverlay() {
  const state = useSyncExternalStore(
    subscribeTour,
    getTourSnapshot,
    getTourSnapshot,
  )
  const { t } = useI18n()
  const location = useLocation()
  const titleId = useId()
  const activeRef = useRef(false)
  activeRef.current = state.active
  const cardRef = useRef<HTMLDivElement>(null)
  const primaryRef = useRef<HTMLButtonElement>(null)
  const holdTimerRef = useRef(0)
  const holdOriginRef = useRef<{ x: number; y: number } | null>(null)
  const holdFiredRef = useRef(false)
  const [holding, setHolding] = useState(false)
  const [view, setView] = useState<{
    hole: typeof EMPTY_HOLE
    cardPos: { top: number; left: number; placement: TourPlacement }
    ready: boolean
  }>({
    hole: EMPTY_HOLE,
    cardPos: CARD_POS_FALLBACK,
    ready: false,
  })
  const [pulse, setPulse] = useState(false)
  const viewRef = useRef(view)
  viewRef.current = view
  const stepRef = useRef(state.step)
  stepRef.current = state.step

  const commitView = useCallback((next: typeof view) => {
    const prev = viewRef.current
    if (
      sameTourHole(prev.hole, next.hole) &&
      sameTourCardPos(prev.cardPos, next.cardPos) &&
      prev.ready === next.ready
    ) {
      return
    }
    viewRef.current = next
    setView(next)
  }, [])

  const measureNow = useCallback(() => {
    const step = stepRef.current
    if (!activeRef.current || !step) return
    const vw = window.innerWidth
    const vh = window.innerHeight
    const rem = rootFontSizePx()
    const isLibraryStep =
      step.id === 'home-widget-library' || step.anchor === 'home-widget-library'
    const isControlPanelStep = step.anchor === 'control-panel'
    const isControlIslandStep = step.anchor === 'control-island'
    const predicted = isPredictedTourAnchor(step.anchor, step.id)
    const found = predicted && !isControlPanelStep
      ? null
      : queryTourAnchor(step.anchor)
    const node = found ? resolveTourMeasureNode(step.anchor, found) : null
    const contentHeight = isControlPanelStep
      ? readControlPanelContentHeight(found)
      : 0
    const box = isLibraryStep
      ? predictedLibraryDockTourBox(vw, vh, rem)
      : isControlPanelStep && contentHeight > 0
        ? predictedControlPanelTourBox(vw, contentHeight, rem)
        : isControlIslandStep
          ? predictedControlIslandTourBox(vw, rem)
          : node
            ? readTourBox(node)
            : null
    if (!box || isDegenerateBox(box)) {
      if (!predicted) recoverTourStep()
      const measured = cardRef.current
      commitView({
        hole: EMPTY_HOLE,
        cardPos: dockTourCard(
          measured?.offsetWidth || CARD_FALLBACK.w,
          measured?.offsetHeight || CARD_FALLBACK.h,
          vw,
          vh,
          TOUR_VIEWPORT_PAD,
        ),
        ready: true,
      })
      return
    }
    const pad = holePadForTourAnchor(step.anchor, box)
    const inflated = inflateRect(box, pad)
    const predictedRadius = isControlPanelStep
      ? predictedControlPanelHoleRadius(rem)
      : isControlIslandStep
        ? predictedControlIslandHoleRadius(rem)
        : node
          ? readRadius(node)
          : 16
    const measured = cardRef.current
    commitView({
      hole: {
        top: inflated.top,
        left: inflated.left,
        width: inflated.width,
        height: inflated.height,
        radius: holeRadiusFor(predictedRadius + pad, inflated),
      },
      cardPos: computeTourCardPosition(
        inflated,
        measured?.offsetWidth || CARD_FALLBACK.w,
        measured?.offsetHeight || CARD_FALLBACK.h,
        vw,
        vh,
      ),
      ready: true,
    })
  }, [commitView])

  const measureRafRef = useRef(0)
  const scheduleMeasure = useCallback(() => {
    if (measureRafRef.current) return
    measureRafRef.current = window.requestAnimationFrame(() => {
      measureRafRef.current = 0
      measureNow()
    })
  }, [measureNow])

  useEffect(() => {
    if (!activeRef.current) return
    stopTour('abort')
  }, [location.pathname])

  useLayoutEffect(() => {
    if (!state.active) {
      commitView({
        hole: EMPTY_HOLE,
        cardPos: CARD_POS_FALLBACK,
        ready: false,
      })
      return
    }
    measureNow()
    const step = state.step
    const predicted = step
      ? isPredictedTourAnchor(step.anchor, step.id)
      : false
    const hosts: HTMLElement[] = []
    // 预计算步不观察 DOM：外壳 morph、内容高度过渡都会每帧触发 RO。
    if (step && !predicted) {
      if (cardRef.current) hosts.push(cardRef.current)
      if (tourMeasureWatchesHost(step.anchor)) {
        const anchor = queryTourAnchor(step.anchor)
        if (anchor) {
          hosts.push(anchor)
          const nav = anchor.closest<HTMLElement>('.nav-container')
          if (nav && nav !== anchor) hosts.push(nav)
        }
      }
    }
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(() => scheduleMeasure())
    if (observer) {
      for (const host of hosts) observer.observe(host)
    }
    const onTransitionEnd = (event: TransitionEvent) => {
      if (
        event.propertyName === 'transform' ||
        event.propertyName === 'width' ||
        event.propertyName === 'height' ||
        event.propertyName === 'top' ||
        event.propertyName === 'left'
      ) {
        scheduleMeasure()
      }
    }
    if (!predicted) {
      for (const host of hosts) {
        host.addEventListener('transitionend', onTransitionEnd)
      }
    }
    const raf1 = window.requestAnimationFrame(measureNow)
    return () => {
      observer?.disconnect()
      if (!predicted) {
        for (const host of hosts) {
          host.removeEventListener('transitionend', onTransitionEnd)
        }
      }
      window.cancelAnimationFrame(raf1)
      if (measureRafRef.current) {
        window.cancelAnimationFrame(measureRafRef.current)
        measureRafRef.current = 0
      }
    }
  }, [commitView, measureNow, scheduleMeasure, state.active, state.step])

  useEffect(() => {
    if (!state.active || !state.step) return
    setPulse(true)
    const timer = window.setTimeout(setPulse, PULSE_MS, false)
    return () => window.clearTimeout(timer)
  }, [state.active, state.step?.id])

  useEffect(() => {
    if (!state.active) return
    const predicted = state.step
      ? isPredictedTourAnchor(state.step.anchor, state.step.id)
      : false
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        stopTour()
      }
    }
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('resize', scheduleMeasure)
    if (!predicted && tourMeasureWatchesScroll()) {
      window.addEventListener('scroll', scheduleMeasure, true)
    }
    if (state.step?.anchor === 'control-panel') {
      window.addEventListener('gcp-animation-end', scheduleMeasure)
      window.addEventListener(
        'control-panel-content-resize',
        scheduleMeasure,
      )
    }
    return () => {
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('resize', scheduleMeasure)
      window.removeEventListener('scroll', scheduleMeasure, true)
      window.removeEventListener('gcp-animation-end', scheduleMeasure)
      window.removeEventListener(
        'control-panel-content-resize',
        scheduleMeasure,
      )
    }
  }, [state.active, state.step, scheduleMeasure])

  useEffect(() => {
    if (!state.active) return
    primaryRef.current?.focus({ preventScroll: true })
  }, [state.active, state.step?.id])

  const endHold = useCallback(() => {
    if (holdTimerRef.current) {
      window.clearTimeout(holdTimerRef.current)
      holdTimerRef.current = 0
    }
    holdOriginRef.current = null
    setHolding(false)
  }, [])

  useEffect(() => () => endHold(), [endHold])
  useEffect(() => {
    endHold()
    holdFiredRef.current = false
  }, [endHold, state.step?.id])

  if (!state.active || !state.step || typeof document === 'undefined') {
    return null
  }

  const copy = stepCopy(t.tour.steps, state.step.id)
  const isLast = state.index >= state.total - 1
  const isFirst = state.index <= 0
  const { hole, cardPos, ready } = view
  const hasHole = hole.width > 0 && hole.height > 0
  const holeStyle = {
    top: hole.top,
    left: hole.left,
    width: hole.width,
    height: hole.height,
    borderRadius: hole.radius,
  }
  const cardStyle = {
    top: cardPos.top,
    left: cardPos.left,
  }

  return createPortal(
    <div
      className="tour-overlay"
      data-hole-sync={tourHoleSync(state.step)}
      data-hole-snap={
        isPredictedTourAnchor(state.step.anchor, state.step.id)
          ? ''
          : undefined
      }
      role="presentation"
    >
      {hasHole ? (
        <>
          <div className="tour-spotlight" style={holeStyle} aria-hidden />
          <div
            className={`tour-ring${pulse ? ' is-pulse' : ''}`}
            style={holeStyle}
            aria-hidden
          />
        </>
      ) : null}
      <div
        ref={cardRef}
        className={`tour-card${ready ? ' is-ready' : ''}`}
        data-placement={cardPos.placement}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-hidden={!ready}
        style={cardStyle}
      >
        <div className="tour-card__copy" key={state.step.id}>
          <h2 id={titleId} className="tour-card__title">
            {copy.title}
          </h2>
          <p className="tour-card__body">{copy.body}</p>
        </div>
        <div className="tour-card__actions">
          {isFirst ? (
            <span className="tour-card__btn-slot" aria-hidden />
          ) : (
            <button
              type="button"
              className="tour-card__btn tour-card__btn--entry tour-card__btn--entry-back"
              aria-label={t.tour.back}
              onClick={previousTourStep}
            >
              <LuChevronLeft aria-hidden />
            </button>
          )}
          {state.total > 1 ? (
            <div
              className="tour-card__progress"
              role="img"
              aria-label={t.tour.stepOf
                .replace('{current}', String(state.index + 1))
                .replace('{total}', String(state.total))}
            >
              {Array.from({ length: state.total }, (_, i) => (
                <span
                  key={i}
                  className={
                    i === state.index
                      ? 'tour-card__pip is-current'
                      : i < state.index
                        ? 'tour-card__pip is-done'
                        : 'tour-card__pip'
                  }
                />
              ))}
            </div>
          ) : (
            <span className="tour-card__actions-spacer" />
          )}
          <button
            ref={primaryRef}
            type="button"
            className="tour-card__btn tour-card__btn--entry tour-card__btn--entry-go"
            data-kind={isLast ? 'done' : 'next'}
            data-holding={holding ? 'true' : undefined}
            title={t.tour.skipHold}
            aria-label={`${isLast ? t.tour.done : t.tour.next}. ${t.tour.skipHold}`}
            onPointerDown={(event) => {
              if (event.button !== 0) return
              holdFiredRef.current = false
              holdOriginRef.current = { x: event.clientX, y: event.clientY }
              event.currentTarget.setPointerCapture(event.pointerId)
              setHolding(true)
              holdTimerRef.current = window.setTimeout(() => {
                holdFiredRef.current = true
                holdTimerRef.current = 0
                setHolding(false)
                if (navigator.vibrate) navigator.vibrate(50)
                stopTour('skip')
              }, SKIP_HOLD_MS)
            }}
            onPointerMove={(event) => {
              const origin = holdOriginRef.current
              if (!origin || holdFiredRef.current) return
              if (
                Math.abs(event.clientX - origin.x) > SKIP_HOLD_SLOP ||
                Math.abs(event.clientY - origin.y) > SKIP_HOLD_SLOP
              ) {
                endHold()
              }
            }}
            onPointerUp={() => endHold()}
            onPointerCancel={() => {
              holdFiredRef.current = false
              endHold()
            }}
            onContextMenu={(event) => event.preventDefault()}
            onClick={(event) => {
              if (holdFiredRef.current) {
                event.preventDefault()
                holdFiredRef.current = false
                return
              }
              if (isLast) stopTour('done')
              else nextTourStep()
            }}
          >
            <span className="tour-card__hold" aria-hidden>
              <svg viewBox="0 0 36 36">
                <circle
                  className="tour-card__hold-track"
                  cx="18"
                  cy="18"
                  r="15"
                  pathLength="100"
                />
                <circle
                  className="tour-card__hold-ring"
                  cx="18"
                  cy="18"
                  r="15"
                  pathLength="100"
                />
              </svg>
            </span>
            {isLast ? <LuCheck aria-hidden /> : <LuArrowRight aria-hidden />}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}
