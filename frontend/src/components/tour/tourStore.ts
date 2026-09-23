import type { TourStepDef } from './tourTypes'
import { createStore } from '../../utils/store'
import { setTourDomActive } from './tourDom'
import { markTourDone } from './tourDone'

export interface TourSnapshot {
  active: boolean
  tourId: string | null
  index: number
  total: number
  step: TourStepDef | null
}

export const EMPTY_TOUR_SNAPSHOT: TourSnapshot = {
  active: false,
  tourId: null,
  index: 0,
  total: 0,
  step: null,
}

let visible: TourStepDef[] = []
const snapshot = createStore(EMPTY_TOUR_SNAPSHOT)

export function emitTourSnapshot(next: TourSnapshot): void {
  snapshot.set(next)
}

export function getTourVisibleSteps(): TourStepDef[] {
  return visible
}

export function setTourVisibleSteps(steps: TourStepDef[]): void {
  visible = steps
}

export function stopTourInternal(): void {
  visible = []
  setTourDomActive(false)
  emitTourSnapshot(EMPTY_TOUR_SNAPSHOT)
}

export const subscribeTour = snapshot.subscribe
export const getTourSnapshot = snapshot.get

export type StopTourReason = 'done' | 'skip' | 'abort'

export function stopTour(reason: StopTourReason = 'abort'): void {
  const current = snapshot.get()
  if (!current.active) return
  if ((reason === 'done' || reason === 'skip') && current.tourId) {
    markTourDone(current.tourId)
  }
  stopTourInternal()
}
