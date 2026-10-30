import type { MoodTransition } from '../performance/types'

export interface MeropeStateEventDetail {
  mood: MoodTransition
  activity: string
}

let currentState: MeropeStateEventDetail | null = null

export function currentMeropeState(): MeropeStateEventDetail | null {
  return currentState
}

export function resetMeropeState(): void {
  currentState = null
}

export function writeMeropeState(detail: MeropeStateEventDetail): void {
  currentState = detail
}
