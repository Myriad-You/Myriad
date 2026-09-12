import type { Anime25DDriver } from './driver'
import type { Anime25DFade } from './types'

export type StylizedExpressionKey = Extract<
  keyof Anime25DDriver,
  'anger' | 'speechless' | 'maniac' | 'silly' | 'lovestruck'
>

export interface StylizedExpressionDefinition {
  priority: number
  fades: readonly Anime25DFade[]
  speechOwnership: 'preserve-articulation' | 'replace-when-silent'
}

export const STYLIZED_EXPRESSION_DEFINITIONS = {
  maniac: {
    priority: 5,
    fades: ['maniacEyeShadow', 'maniacMouthShadow', 'mouthManiac'],
    speechOwnership: 'replace-when-silent',
  },
  silly: {
    priority: 4,
    fades: ['eyeSilly', 'mouthSilly'],
    speechOwnership: 'replace-when-silent',
  },
  lovestruck: {
    priority: 3,
    fades: ['lovestruckHeart', 'lovestruckFace', 'lovestruckDrool'],
    speechOwnership: 'preserve-articulation',
  },
  anger: {
    priority: 2,
    fades: ['angerMark'],
    speechOwnership: 'preserve-articulation',
  },
  speechless: {
    priority: 1,
    fades: ['speechlessSweat'],
    speechOwnership: 'preserve-articulation',
  },
} as const satisfies Record<StylizedExpressionKey, StylizedExpressionDefinition>

export const STYLIZED_EXPRESSION_PRECEDENCE = (
  Object.keys(STYLIZED_EXPRESSION_DEFINITIONS) as StylizedExpressionKey[]
).toSorted(
  (left, right) =>
    STYLIZED_EXPRESSION_DEFINITIONS[right].priority -
    STYLIZED_EXPRESSION_DEFINITIONS[left].priority,
)

export type StylizedExpressionTargets = Record<StylizedExpressionKey, number>

export function resolveStylizedExpressionTargets(
  input: Readonly<StylizedExpressionTargets>,
  output: StylizedExpressionTargets,
): void {
  let remaining = 1
  for (const key of STYLIZED_EXPRESSION_PRECEDENCE) {
    const selected = clamp(input[key], 0, 1) * remaining
    output[key] = selected
    remaining *= 1 - selected
  }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
