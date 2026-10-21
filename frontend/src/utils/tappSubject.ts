import { emitAppEvent } from './appEvents'
import { createStore, useStore } from './store'

const tappSubject = createStore({ epoch: 0, ready: true })
export const getTappSubjectSnapshot = tappSubject.get
export const subscribeTappSubject = tappSubject.subscribe

/** Identity changes, unlike visibility changes, invalidate every old TAPP consumer. */
export function beginTappSubjectChange(): number {
  tappSubject.set(({ epoch }) => ({ epoch: epoch + 1, ready: false }))
  return tappSubject.get().epoch
}

export function finishTappSubjectChange(epoch: number, isAuthenticated: boolean): void {
  if (epoch !== tappSubject.get().epoch) return
  tappSubject.set({ epoch, ready: true })
  emitAppEvent('tapp-subject-ready', { isAuthenticated })
}

export function useTappSubject() {
  return useStore(tappSubject)
}
