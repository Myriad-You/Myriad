import type { CueIntent, PerformanceCueFootprint } from '../motion/performanceCues'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset } from './performanceExpression'
import { PERFORMANCE_CUE_FOOTPRINTS } from '../motion/performanceCues'
import { IDENTITY_DRIVER } from './driver'

export { cueIsSticker, performanceCueChannels } from '../motion/performanceCues'
export type { CueIntent } from '../motion/performanceCues'

/** How a cue moves her; what it occupies is planning's (`PerformanceCueFootprint`). */
interface PerformanceCueMotion {
  driver: (amount: number) => Partial<Anime25DDriver>
  expression: (
    amount: number,
    poseAmount: number,
  ) => Partial<PerformanceExpressionOffset>
}

const PERFORMANCE_CUE_MOTION = {
  greet: {
    driver: (poseAmount) => ({
      body: 0.22 * poseAmount * bodyParticipation(poseAmount),
      armY: 0.3 * poseAmount * bodyParticipation(poseAmount),
    }),
    expression: (amount, poseAmount) => ({
      angleZ: -0.11 * poseAmount,
      brow: 0.17 * amount,
    }),
  },
  respond: {
    driver: (poseAmount) => ({ body: 0.2 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleY: -0.12 * poseAmount,
      angleZ: -0.045 * poseAmount,
      brow: 0.15 * amount,
    }),
  },
  question: {
    driver: (poseAmount) => ({ body: 0.18 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleZ: 0.15 * poseAmount,
      brow: 0.26 * amount,
      eyeOpen: 0.05 * amount,
      eyeWide: 0.2 * amount,
    }),
  },
  delight: {
    driver: (poseAmount) => ({
      body: 0.16 * poseAmount * bodyParticipation(poseAmount),
      armY: 0.4 * poseAmount * bodyParticipation(poseAmount),
      bust: IDENTITY_DRIVER.bust + 0.26 * poseAmount,
    }),
    expression: (amount, poseAmount) => ({
      angleY: -0.12 * poseAmount,
      brow: 0.22 * amount,
      // A smile that reaches the eyes, not the squeezed >< artwork: that is a
      // comic symbol and jars on anything but flat cel faces.
      eyeOpen: -0.12 * amount,
      eyeSmile: 0.9 * amount,
      mouthForm: 0.18 * amount,
    }),
  },
  emphasize: {
    driver: (poseAmount) => ({
      body: 0.4 * poseAmount * bodyParticipation(poseAmount),
    }),
    expression: (amount, poseAmount) => ({
      angleY: 0.12 * poseAmount,
      brow: 0.2 * amount,
    }),
  },
  listen: {
    driver: (poseAmount) => ({ body: 0.12 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleY: 0.09 * poseAmount,
      brow: 0.12 * amount,
    }),
  },
  notify: {
    driver: (poseAmount) => ({ body: 0.32 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleZ: -0.11 * poseAmount,
      brow: 0.22 * amount,
      eyeOpen: 0.055 * amount,
      eyeWide: 0.35 * amount,
    }),
  },
  think: {
    driver: () => ({}),
    expression: (amount, poseAmount) => ({
      angleZ: -0.2 * poseAmount,
      brow: 0.24 * amount,
      browAngSym: -0.18 * amount,
      eyeOpen: -0.16 * amount,
      irisScale: -0.06 * amount,
      mouthForm: -0.16 * amount,
      eyeX: 0.5 * amount,
      eyeY: -0.36 * amount,
    }),
  },
  dizzy: {
    driver: () => ({}),
    expression: () => ({ eyeDizzy: 1 }),
  },
  cry: {
    driver: () => ({}),
    expression: (amount) => ({
      brow: 0.2 * amount,
      browAngSym: -0.3 * amount,
      eyeCry: 1,
      mouthForm: -0.12 * amount,
    }),
  },
  angry: {
    driver: (poseAmount) => ({ body: 0.22 * poseAmount }),
    expression: (amount) => ({ anger: amount }),
  },
  speechless: {
    driver: (poseAmount) => ({ body: -0.18 * poseAmount }),
    expression: (amount) => ({ speechless: amount }),
  },
  maniac: {
    driver: (poseAmount) => ({ body: 0.17 * poseAmount }),
    expression: (amount) => ({ maniac: amount }),
  },
  silly: {
    driver: (poseAmount) => ({ body: -0.15 * poseAmount }),
    expression: (amount) => ({ silly: amount }),
  },
  lovestruck: {
    driver: (poseAmount) => ({ body: -0.14 * poseAmount }),
    expression: (amount) => ({ lovestruck: amount }),
  },
} satisfies Record<CueIntent, PerformanceCueMotion>

export type PerformanceCueDefinition = PerformanceCueFootprint & PerformanceCueMotion

/** Each cue whole: its footprint and its motion, joined once. */
export const PERFORMANCE_CUE_DEFINITIONS = Object.fromEntries(
  (Object.keys(PERFORMANCE_CUE_MOTION) as CueIntent[]).map(
    (intent): [CueIntent, PerformanceCueDefinition] => [
      intent,
      { ...PERFORMANCE_CUE_FOOTPRINTS[intent], ...PERFORMANCE_CUE_MOTION[intent] },
    ],
  ),
) as Record<CueIntent, PerformanceCueDefinition>

export function performanceCueDefinition(
  intent: CueIntent,
): PerformanceCueDefinition {
  return PERFORMANCE_CUE_DEFINITIONS[intent]
}

export function intentExpressionPatch(
  intent: CueIntent,
  intensity: number,
): Partial<PerformanceExpressionOffset> {
  const definition = performanceCueDefinition(intent)
  const amount = intentAmount(intensity)
  const poseAmount = intentPoseAmount(intensity)
  const driver = definition.driver(poseAmount)
  return {
    ...definition.expression(amount, poseAmount),
    ...(typeof driver.body === 'number' ? { body: driver.body } : {}),
    ...(typeof driver.armY === 'number' ? { armY: driver.armY } : {}),
    ...(typeof driver.armPos === 'number' ? { armPos: driver.armPos } : {}),
    ...(typeof driver.bust === 'number'
      ? { bust: driver.bust - IDENTITY_DRIVER.bust }
      : {}),
  }
}

function intentAmount(intensity: number): number {
  return Math.max(0.2, Math.min(1.4, intensity))
}

export function intentPoseAmount(intensity: number): number {
  const normalized = (intentAmount(intensity) - 0.2) / 1.2
  return 0.72 + normalized * 0.68
}

/** Strong greetings/joy/emphasis recruit the body, not extra head pitch or face. */
function bodyParticipation(poseAmount: number): number {
  const t = Math.max(0, Math.min(1, (poseAmount - 0.9) / 0.5))
  return 1 + 0.5 * t * t * (3 - 2 * t)
}
