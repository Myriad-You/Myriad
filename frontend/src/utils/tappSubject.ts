import { useSyncExternalStore } from 'react'
import { emitAppEvent } from './appEvents'

let snapshot = { epoch: 0, ready: true }
const listeners = new Set<() => void>()
function subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener) } }
export const getTappSubjectSnapshot = () => snapshot

/** Identity changes, unlike visibility changes, invalidate every old TAPP consumer. */
export function beginTappSubjectChange(): number {
  snapshot = { epoch: snapshot.epoch + 1, ready: false }
  listeners.forEach(listener => listener())
  return snapshot.epoch
}

export function finishTappSubjectChange(epoch: number, isAuthenticated: boolean): void {
  if (epoch !== snapshot.epoch) return
  snapshot = { epoch, ready: true }
  listeners.forEach(listener => listener())
  emitAppEvent('tapp-subject-ready', { isAuthenticated })
}

export function useTappSubject() {
  return useSyncExternalStore(subscribe, getTappSubjectSnapshot, getTappSubjectSnapshot)
}
