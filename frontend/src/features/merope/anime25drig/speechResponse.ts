const MOUTH_ATTACK_RATE = 20
const MOUTH_RELEASE_RATE = 10
const MOUTH_FORM_RATE = 10
const RESPONSE_EPSILON = 1e-5

/**
 * Present speech targets with a quick opening and a softer return to rest.
 * The exponential step is time-based, so identical target timelines produce
 * the same response at common rendering frame rates.
 */
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

/** Mouth shape changes more slowly than jaw opening to avoid corner twitch. */
export function stepMouthForm(
  current: number,
  target: number,
  deltaSeconds: number,
): number {
  return stepExponential(current, target, deltaSeconds, MOUTH_FORM_RATE)
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
