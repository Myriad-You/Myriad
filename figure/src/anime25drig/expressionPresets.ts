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

export const THINKING_ACTIVITY_EXPRESSION: Readonly<ActivityExpressionDriver> =
  {
    angleX: -0.12,
    angleY: 0.1,
    angleZ: -0.18,
    eyeOpenL: 0.78,
    eyeOpenR: 0.9,
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
    irisScale: 0.94,
    brow: 0.24,
    browAngL: 0.48,
    browAngR: -0.12,
    browAngSym: 0,
  }

export const DIZZY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  eyeDizzy: 1,
}

export const SQUEEZE_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  eyeSqueeze: 1,
}

export const CRY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  eyeCry: 1,
  brow: 0.28,
  browAngSym: -0.34,
}

export const ANGRY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 1,
  speechless: 0,
}

export const SPEECHLESS_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 1,
}

export const MANIAC_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 0,
  maniac: 1,
  eyeOpenL: 1,
  eyeOpenR: 0.94,
}

export const SILLY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>> = {
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 1,
}

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
  mouthOpen: 0,
  mouthForm: -0.16,
}

export function activityExpressionDriverPatch(
  thinking: boolean,
): Readonly<ActivityExpressionDriver> {
  return thinking ? THINKING_ACTIVITY_EXPRESSION : NEUTRAL_ACTIVITY_EXPRESSION
}

type ThinkingExpressionKey = keyof ActivityExpressionDriver | 'mouthForm'
const THINKING_EXPRESSION_KEYS: readonly ThinkingExpressionKey[] = [
  ...Object.keys(THINKING_ACTIVITY_EXPRESSION) as (keyof ActivityExpressionDriver)[],
  'mouthForm',
]

/** Ownership follows writes, never numeric coincidence with a preset. */
export class ThinkingExpressionOwnership {
  private readonly previous = new Map<ThinkingExpressionKey, number>()

  apply(target: Anime25DDriver, patch: Partial<Anime25DDriver>, speaking: boolean): void {
    if (patch.thinking === false) this.release(target)
    for (const key of THINKING_EXPRESSION_KEYS) {
      if (!Object.hasOwn(patch, key)) continue
      if (patch.thinking === true) {
        if (!this.previous.has(key)) this.previous.set(key, target[key])
      } else {
        // Even writing the same number is an explicit takeover.
        this.previous.delete(key)
      }
    }
    Object.assign(target, patch)
    if (speaking) this.release(target)
  }

  release(target: Anime25DDriver): void {
    for (const [key, value] of this.previous) target[key] = value
    this.previous.clear()
    target.thinking = false
  }

  clear(): void { this.previous.clear() }
}
