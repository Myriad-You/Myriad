import type { Anime25DDriver } from './driver'

type ActivityExpressionDriver = Pick<
  Anime25DDriver,
  | 'angleX'
  | 'angleY'
  | 'angleZ'
  | 'eyeOpenL'
  | 'eyeOpenR'
  | 'eyeDizzy'
  | 'eyeSqueeze'
  | 'eyeCry'
  | 'anger'
  | 'speechless'
  | 'maniac'
  | 'silly'
  | 'lovestruck'
  | 'eyeX'
  | 'eyeY'
  | 'irisScale'
  | 'brow'
  | 'browAngL'
  | 'browAngR'
  | 'browAngSym'
>

const NEUTRAL_ACTIVITY_EXPRESSION: Readonly<ActivityExpressionDriver> = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  eyeOpenL: 1,
  eyeOpenR: 1,
  eyeDizzy: 0,
  eyeSqueeze: 0,
  eyeCry: 0,
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 0,
  eyeX: 0,
  eyeY: 0,
  irisScale: 1,
  brow: 0,
  browAngL: 0,
  browAngR: 0,
  browAngSym: 0,
}

/** Persistent, speech-safe face pose while the Agent is actually thinking. */
export const THINKING_ACTIVITY_EXPRESSION: Readonly<ActivityExpressionDriver> =
  {
    angleX: -0.12,
    angleY: 0.1,
    angleZ: -0.18,
    eyeOpenL: 1,
    eyeOpenR: 1,
    eyeDizzy: 0,
    eyeSqueeze: 0,
    eyeCry: 0,
    anger: 0,
    speechless: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
    eyeX: 0.58,
    eyeY: -0.42,
    irisScale: 1,
    brow: 0.18,
    browAngL: 0.16,
    browAngR: 0,
    browAngSym: 0,
  }

/** Dedicated artwork replacement; no unrelated facial or body channel is changed. */
export const DIZZY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  eyeDizzy: 1,
}

/** Dedicated inward-facing chevron eye artwork, independent of blink state. */
export const SQUEEZE_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  eyeSqueeze: 1,
}

/** Complete per-eye crying artwork; runtime adds restrained sobbing mouth motion. */
export const CRY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  eyeCry: 1,
  brow: 0.28,
  browAngSym: -0.34,
}

/** Facial deformation carries the expression; the vein mark is secondary. */
export const ANGRY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 1,
  speechless: 0,
}

/** Half-lidded side glance with a restrained temple sweat accent. */
export const SPEECHLESS_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 1,
}

/** Wide open laugh with rolled-up gaze; runtime adds restrained instability. */
export const MANIAC_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 0,
  maniac: 1,
  eyeOpenL: 1,
  eyeOpenR: 0.94,
}

/**
 * Vacant stare with its own eye artwork. The two irides drift apart on the
 * expression's private schedule, so no shared gaze channel is touched here.
 */
export const SILLY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 1,
}

/** Half-lidded heart pupils with a face-local blush and restrained soft mouth. */
export const LOVESTRUCK_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 1,
  eyeOpenL: 1,
  eyeOpenR: 1,
}

export const THINKING_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  ...THINKING_ACTIVITY_EXPRESSION,
  thinking: true,
  angleX: -0.15,
  angleY: 0.12,
  angleZ: -0.22,
  eyeX: 0.68,
  eyeY: -0.48,
  brow: 0.22,
  browAngL: 0.2,
  mouthOpen: 0,
  mouthForm: 0,
}

export function activityExpressionDriverPatch(
  thinking: boolean,
): Readonly<ActivityExpressionDriver> {
  return thinking ? THINKING_ACTIVITY_EXPRESSION : NEUTRAL_ACTIVITY_EXPRESSION
}
