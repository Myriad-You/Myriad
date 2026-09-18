import type { TourDefinition, TourStepDef, TourSurfacePick } from './tourTypes'
import {
  firstVisibleIndex,
  isTourStepAvailable,
  nextIndexAfterTourAction,
  previousVisibleIndex,
  revealTourAnchor,
  tourStepBlocksAdvance,
} from './tourLogic'
import { pickRegisteredTour } from './tourRegistry'
import { setTourDomActive } from './tourDom'
import {
  emitTourSnapshot,
  getTourSnapshot,
  getTourVisibleSteps,
  setTourVisibleSteps,
  stopTour,
  stopTourInternal,
  subscribeTour,
} from './tourStore'

export type { StopTourReason, TourSnapshot } from './tourStore'
export { getTourSnapshot, stopTour, subscribeTour } from './tourStore'

export type StartTourResult = 'started' | 'no-tour' | 'no-targets'

function stepAvailable(_anchor: string, step: TourStepDef): boolean {
  return isTourStepAvailable(step)
}

function showIndex(index: number): void {
  const visible = getTourVisibleSteps()
  const step = visible[index]
  if (!step) {
    stopTourInternal()
    return
  }
  revealTourAnchor(step.anchor, step.id)
  const snapshot = getTourSnapshot()
  emitTourSnapshot({
    active: true,
    tourId: snapshot.tourId,
    index,
    total: visible.length,
    step,
  })
}

export function startTour(def: TourDefinition): boolean {
  const first = firstVisibleIndex(def.steps, stepAvailable, 0)
  if (first < 0) return false
  const step = def.steps[first]!
  revealTourAnchor(step.anchor, step.id)
  setTourVisibleSteps(Iterator.from(def.steps).toArray())
  setTourDomActive(true)
  emitTourSnapshot({
    active: true,
    tourId: def.id,
    index: first,
    total: def.steps.length,
    step,
  })
  return true
}

export function startTourForRoute(
  pathname: string,
  isOwner: boolean,
  surface: TourSurfacePick = 'browse',
): StartTourResult {
  const def = pickRegisteredTour(pathname, isOwner, surface)
  if (!def) return 'no-tour'
  return startTour(def) ? 'started' : 'no-targets'
}

export function nextTourStep(): void {
  const snapshot = getTourSnapshot()
  if (!snapshot.active) return
  if (snapshot.step?.action) {
    completeTourAction()
    return
  }
  const next = firstVisibleIndex(
    getTourVisibleSteps(),
    stepAvailable,
    snapshot.index + 1,
  )
  if (next < 0) {
    stopTour('done')
    return
  }
  showIndex(next)
}

// 紧后介绍步还没挂上就等，不要跨过去。
export function completeTourAction(): void {
  const snapshot = getTourSnapshot()
  if (!snapshot.active || !snapshot.step?.action) return
  if (tourStepBlocksAdvance(snapshot.step)) return
  const next = nextIndexAfterTourAction(
    getTourVisibleSteps(),
    snapshot.index,
    isTourStepAvailable,
  )
  if (next === 'wait') return
  if (next < 0) {
    stopTour('done')
    return
  }
  showIndex(next)
}

export function previousTourStep(): void {
  const snapshot = getTourSnapshot()
  if (!snapshot.active) return
  const prev = previousVisibleIndex(
    getTourVisibleSteps(),
    stepAvailable,
    snapshot.index,
  )
  if (prev < 0) return
  showIndex(prev)
}

// 当前锚消失或量不到就前跳或停。
export function recoverTourStep(): void {
  const snapshot = getTourSnapshot()
  if (!snapshot.active || !snapshot.step) return
  if (isTourStepAvailable(snapshot.step)) return
  if (snapshot.step.after) return
  const next = firstVisibleIndex(
    getTourVisibleSteps(),
    stepAvailable,
    snapshot.index + 1,
  )
  if (next < 0) {
    stopTourInternal()
    return
  }
  showIndex(next)
}
