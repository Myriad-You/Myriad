import { noteTurnTraceDrop } from '../turnTrace'

/**
 * In-memory Chat generation seen by speech and motion producers.
 * Preview / proactive events omit generation and still apply.
 */
let liveGeneration = 0

export function setLiveMotionGeneration(generation: number): void {
  liveGeneration = Math.max(0, Math.trunc(generation))
}

export function liveMotionGeneration(): number {
  return liveGeneration
}

export function isLiveMotionGeneration(generation: number | undefined): boolean {
  if (generation == null || generation <= 0) return true
  return generation === liveGeneration
}

export function acceptLiveMotionGeneration(
  generation: number | undefined,
): boolean {
  if (isLiveMotionGeneration(generation)) return true
  noteTurnTraceDrop('stale_generation')
  return false
}

let intentSeq = 0

export function newMotionIntentId(): string {
  intentSeq += 1
  return `motion-${liveGeneration}-${intentSeq}`
}
