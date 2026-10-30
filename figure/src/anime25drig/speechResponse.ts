const MOUTH_ATTACK_RATE = 20
const MOUTH_RELEASE_RATE = 10
const MOUTH_FORM_RATE = 10
const MOUTH_SHAPE_RATE = 18
const MOUTH_SEAL_ATTACK_RATE = 32
const MOUTH_SEAL_RELEASE_RATE = 19
const RESPONSE_EPSILON = 1e-5

export function stepMouthOpen(
  current: number,
  target: number,
  deltaSeconds: number,
): number {
  return stepExponential(
    current,
    target,
    deltaSeconds,
    target > current ? MOUTH_ATTACK_RATE : MOUTH_RELEASE_RATE,
  )
}

export function stepMouthForm(
  current: number,
  target: number,
  deltaSeconds: number,
): number {
  return stepExponential(current, target, deltaSeconds, MOUTH_FORM_RATE)
}

/** Articulatory sprite weights should track speech quickly without hard cuts. */
export function stepMouthShape(
  current: number,
  target: number,
  deltaSeconds: number,
): number {
  return stepExponential(current, target, deltaSeconds, MOUTH_SHAPE_RATE)
}

/** Bilabials close the lips quickly without forcing the jaw to snap shut. */
export function stepMouthSeal(
  current: number,
  target: number,
  deltaSeconds: number,
): number {
  return stepExponential(
    current,
    target,
    deltaSeconds,
    target > current ? MOUTH_SEAL_ATTACK_RATE : MOUTH_SEAL_RELEASE_RATE,
  )
}

function stepExponential(
  current: number,
  target: number,
  deltaSeconds: number,
  rate: number,
): number {
  if (!Number.isFinite(current) || !Number.isFinite(target)) return target
  const difference = target - current
  if (Math.abs(difference) <= RESPONSE_EPSILON) return target
  const dt = Number.isFinite(deltaSeconds) ? Math.max(0, deltaSeconds) : 0
  if (dt === 0) return current
  return current + difference * (1 - Math.exp(-rate * dt))
}
