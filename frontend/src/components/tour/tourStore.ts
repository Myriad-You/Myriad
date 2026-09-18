import type { TourStepDef } from './tourTypes'
import { markTourDone } from './tourDone'
import { setTourDomActive } from './tourDom'

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
let snapshot: TourSnapshot = EMPTY_TOUR_SNAPSHOT
const listeners = new Set<() => void>()

export function emitTourSnapshot(next: TourSnapshot): void {
  snapshot = next
  listeners.forEach((listener) => listener())
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

export function subscribeTour(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange)
  return () => {
    listeners.delete(onStoreChange)
  }
}

export function getTourSnapshot(): TourSnapshot {
  return snapshot
}

export type StopTourReason = 'done' | 'skip' | 'abort'

export function stopTour(reason: StopTourReason = 'abort'): void {
  if (!snapshot.active) return
  if ((reason === 'done' || reason === 'skip') && snapshot.tourId) {
    markTourDone(snapshot.tourId)
  }
  stopTourInternal()
}
