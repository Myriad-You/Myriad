export const MAX_ANIMATION_STEP_SECONDS = 0.05
export const MAX_ANIMATION_CATCHUP_SECONDS = 0.25

export function animationElapsedSeconds(deltaSeconds: number): number {
  if (!Number.isFinite(deltaSeconds)) return 0.001
  return Math.max(0.001, deltaSeconds)
}

export function animationCatchupSeconds(elapsedSeconds: number): number {
  return Math.min(
    Math.max(0.001, elapsedSeconds),
    MAX_ANIMATION_CATCHUP_SECONDS,
  )
}

export function animationSubstepCount(catchupSeconds: number): number {
  return Math.max(1, Math.ceil(catchupSeconds / MAX_ANIMATION_STEP_SECONDS))
}
