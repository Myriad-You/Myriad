import type { TouchReaction } from '../interaction/touchReaction'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { PerformanceExpressionOffset } from './performanceExpression'

export function touchExpressionPatch(form: TouchReaction, amount: number,
  contact: Anime25DMotionUnit['touch'] = undefined, age = 0): Partial<PerformanceExpressionOffset> {
  const bound = (n: number) => Number.isFinite(n) ? Math.max(-1, Math.min(1, n)) : 0
  const x = bound(contact?.x ?? 0)
  const y = bound(contact?.y ?? 0)
  const strokeX = bound(contact?.strokeX ?? 0)
  const strokeY = bound(contact?.strokeY ?? 0)
  const elapsed = Math.max(0, Number.isFinite(age) ? age : 0)
  // A single orienting response settles rather than looping or growing forever.
  const settled = 1 - Math.exp(-elapsed / 1.8)
  const strength = amount * (1 - 0.25 * settled)
  // Not a looping oscillator
  const phraseTime = Math.max(0, elapsed - 0.12) / 0.38
  const answer = phraseTime * phraseTime * Math.exp(2 - 2 * phraseTime)
  const response = answer * amount
  // Only acceptance softens into the movement.
  const caress = Math.max(0, bound(contact?.caress ?? 0)) * amount
  const toward = form === 'withdraw' ? -1 : form === 'hesitate' ? -0.4 : 1
  const spatial = {
    eyeX: x * amount * (0.32 - 0.12 * settled),
    eyeY: -y * amount * (0.18 - 0.06 * settled),
    angleZ: toward * (x * 0.16 + strokeX * 0.09) * strength,
  }
  switch (form) {
    case 'notice': return { ...spatial, angleY: (0.09 - y * 0.04) * strength, brow: 0.48 * strength, eyeOpen: -0.08 * amount, body: 0.08 * strength }
    case 'accept': return { ...spatial, angleY: -strokeY * (0.06 * strength + 0.08 * caress) + 0.12 * response,
      angleZ: spatial.angleZ + strokeX * 0.12 * caress,
      // eyeSqueeze is replacement artwork, not a continuous eyelid control.
      eyeOpen: -(0.46 + 0.16 * settled) * amount - 0.12 * caress,
      brow: -0.2 * strength - 0.08 * caress, browAngSym: -0.26 * amount - 0.1 * caress,
      body: 0.1 * strength + 0.04 * response + 0.08 * caress }
    case 'hesitate': return { ...spatial, angleY: -0.07 * response,
      angleZ: spatial.angleZ - x * 0.08 * response, eyeOpen: -0.3 * amount,
      brow: 0.2 * amount, browAngSym: -0.65 * amount, body: -0.08 * strength }
    case 'withdraw': return { ...spatial, angleY: -0.15 * strength - 0.1 * response,
      eyeOpen: -0.24 * amount, brow: -0.25 * amount, browAngSym: 0.8 * amount, body: -0.2 * strength - 0.05 * response }
  }
}
