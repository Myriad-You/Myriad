import {
  mixBoundedExpressionChannel,
  mixEyeOpen,
} from './performanceExpression'

export type RandomActionName =
  'postureShift' | 'headDrift' | 'shoulderEase' | 'softBlink'

export interface RandomActionFrame {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  eyeX: number
  eyeY: number
  brow: number
  browAngSym: number
  eyeOpen: number
  irisScale: number
  armY: number
  armPos: number
  ambientScale: number
}

interface RandomActionTarget {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  eyeX: number
  eyeY: number
  brow: number
  browAngSym: number
  eyeOpenL: number
  eyeOpenR: number
  irisScale: number
  armY: number
  armPos: number
}

interface ActionDefinition {
  name: RandomActionName
  minimumDuration: number
  maximumDuration: number
  weight?: number
}

type RandomSource = () => number

const IDLE_ACTIONS: readonly ActionDefinition[] = [
  { name: 'postureShift', minimumDuration: 1.8, maximumDuration: 2.3 },
  { name: 'headDrift', minimumDuration: 2.2, maximumDuration: 2.8 },
  { name: 'shoulderEase', minimumDuration: 2.1, maximumDuration: 2.7 },
  {
    name: 'softBlink',
    minimumDuration: 1.6,
    maximumDuration: 2.1,
    weight: 2.4,
  },
]

const NEUTRAL_FRAME: RandomActionFrame = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  body: 0,
  eyeX: 0,
  eyeY: 0,
  brow: 0,
  browAngSym: 0,
  eyeOpen: 0,
  irisScale: 0,
  armY: 0,
  armPos: 0,
  ambientScale: 1,
}

const RELEASE_DURATION = 0.38
const RESUME_DELAY_MIN = 0.24
const RESUME_DELAY_MAX = 0.42

/**
 * Plays complete, low-frequency idle action clips independently from ambient
 * gaze. The output object is reused so this adds no per-frame allocations.
 */
export class RandomActionController {
  private readonly output: RandomActionFrame = { ...NEUTRAL_FRAME }
  private readonly actionFrom: RandomActionFrame = { ...NEUTRAL_FRAME }
  private readonly releaseFrom: RandomActionFrame = { ...NEUTRAL_FRAME }
  private initialized = false
  private available = false
  private activeIndex = -1
  private lastIndex = -1
  private actionStartedAt = 0
  private actionDuration = 1
  private actionDirection = 1
  private actionIntensity = 1
  private nextActionAt = Number.POSITIVE_INFINITY
  private releaseStartedAt = 0
  private releasing = false

  constructor(private readonly random: RandomSource = Math.random) {}

  sample(
    timeSeconds: number,
    enabled: boolean,
    blocked: boolean,
  ): Readonly<RandomActionFrame> {
    const now = finiteTime(timeSeconds)
    const available = enabled && !blocked
    if (!this.initialized) {
      this.initialized = true
      this.available = available
      if (available) this.scheduleFirstAction(now)
    }

    if (!available) {
      if (this.activeIndex >= 0) {
        this.resolveAction(now)
        this.beginRelease(now)
      }
      this.available = false
      this.nextActionAt = Number.POSITIVE_INFINITY
      return this.resolveRelease(now)
    }

    if (!this.available) {
      this.available = true
      this.scheduleResumeAction(now)
    }

    if (this.releasing) this.resolveRelease(now)
    if (this.activeIndex >= 0) {
      if (now < this.actionStartedAt + this.actionDuration) {
        return this.resolveAction(now)
      }
      this.activeIndex = -1
      writeNeutral(this.output)
    }

    if (now >= this.nextActionAt) {
      this.beginAction(now)
      return this.resolveAction(now)
    }
    return this.output
  }

  getActiveAction(): RandomActionName | null {
    return this.activeIndex >= 0
      ? (this.catalog()[this.activeIndex]?.name ?? null)
      : null
  }

  private catalog(): readonly ActionDefinition[] {
    return IDLE_ACTIONS
  }

  private scheduleFirstAction(now: number): void {
    this.nextActionAt = now + this.randomRange(1.2, 2)
  }

  private scheduleResumeAction(now: number): void {
    this.nextActionAt =
      now + this.randomRange(RESUME_DELAY_MIN, RESUME_DELAY_MAX)
  }

  private beginAction(now: number): void {
    this.releasing = false
    this.activeIndex = this.nextActionIndex()
    this.lastIndex = this.activeIndex
    const actions = this.catalog()
    const action = actions[this.activeIndex] ?? actions[0]
    this.actionStartedAt = now
    this.actionDuration = this.randomRange(
      action.minimumDuration,
      action.maximumDuration,
    )
    this.actionDirection = this.randomUnit() < 0.5 ? -1 : 1
    this.actionIntensity = this.randomRange(0.9, 1.08)
    this.nextActionAt = now + this.actionDuration + this.randomRange(3.8, 6.5)
    copyFrame(this.actionFrom, this.output)
  }

  private nextActionIndex(): number {
    const actions = this.catalog()
    if (actions.length <= 1) return 0
    let total = 0
    for (let index = 0; index < actions.length; index += 1) {
      if (index === this.lastIndex) continue
      total += actionWeight(actions[index])
    }
    let pick = this.randomUnit() * total
    for (let index = 0; index < actions.length; index += 1) {
      if (index === this.lastIndex) continue
      pick -= actionWeight(actions[index])
      if (pick <= 0) return index
    }
    return this.lastIndex === 0 ? 1 : 0
  }

  private resolveAction(now: number): Readonly<RandomActionFrame> {
    const action = this.catalog()[this.activeIndex]
    if (!action) return this.output
    const progress = clamp(
      (now - this.actionStartedAt) / this.actionDuration,
      0,
      1,
    )
    const motion = stagedEnvelope(progress, 0.2, 0.68)
    const face = stagedEnvelope(progress, 0.16, 0.7)
    const gesture = stagedEnvelope(progress, 0.24, 0.66)
    const direction = this.actionDirection
    const intensity = this.actionIntensity
    writeNeutral(this.output)

    switch (action.name) {
      case 'postureShift': {
        this.output.angleY = 0.035 * motion * intensity
        this.output.angleZ = direction * 0.05 * motion * intensity
        this.output.body = 0.035 * motion * intensity
        this.output.eyeOpen = -0.035 * face * intensity
        this.output.ambientScale = 1 - 0.32 * motion
        break
      }
      case 'headDrift':
        this.output.angleX = direction * 0.045 * motion * intensity
        this.output.angleY = -0.025 * motion * intensity
        this.output.angleZ = direction * 0.075 * motion * intensity
        this.output.body = -direction * 0.035 * motion * intensity
        this.output.eyeOpen = -0.025 * face * intensity
        this.output.ambientScale = 1 - 0.38 * motion
        break
      case 'shoulderEase':
        this.output.angleX = direction * 0.035 * motion * intensity
        this.output.angleY = -0.025 * motion * intensity
        this.output.angleZ = -direction * 0.045 * motion * intensity
        this.output.body = direction * 0.055 * motion * intensity
        this.output.eyeOpen = -0.035 * face * intensity
        this.output.armY = 0.1 * gesture * intensity
        this.output.armPos = -0.025 * gesture * intensity
        this.output.ambientScale = 1 - 0.4 * motion
        break
      case 'softBlink':
        this.output.angleX = direction * 0.02 * motion * intensity
        this.output.angleZ = direction * 0.04 * motion * intensity
        this.output.body = -direction * 0.025 * motion * intensity
        this.output.eyeOpen = -0.18 * face * intensity
        this.output.armY = 0.035 * gesture * intensity
        this.output.ambientScale = 1 - 0.3 * motion
        break
    }
    this.blendFromPrevious(smootherstep(progress / 0.28))
    return this.output
  }

  private blendFromPrevious(amount: number): void {
    if (amount >= 1) return
    for (const key of ACTION_OFFSET_KEYS) {
      this.output[key] = mix(this.actionFrom[key], this.output[key], amount)
    }
    this.output.ambientScale = mix(
      this.actionFrom.ambientScale,
      this.output.ambientScale,
      amount,
    )
  }

  private beginRelease(now: number): void {
    copyFrame(this.releaseFrom, this.output)
    this.activeIndex = -1
    this.releaseStartedAt = now
    this.releasing = true
  }

  private resolveRelease(now: number): Readonly<RandomActionFrame> {
    if (!this.releasing) {
      writeNeutral(this.output)
      return this.output
    }
    const progress = smootherstep(
      (now - this.releaseStartedAt) / RELEASE_DURATION,
    )
    for (const key of ACTION_OFFSET_KEYS) {
      this.output[key] = this.releaseFrom[key] * (1 - progress)
    }
    this.output.ambientScale = mix(this.releaseFrom.ambientScale, 1, progress)
    if (progress >= 1) {
      this.releasing = false
      writeNeutral(this.output)
    }
    return this.output
  }

  private randomRange(minimum: number, maximum: number): number {
    return minimum + (maximum - minimum) * this.randomUnit()
  }

  private randomUnit(): number {
    const value = this.random()
    return Number.isFinite(value) ? clamp(value, 0, 1) : 0.5
  }
}

function actionWeight(action: ActionDefinition): number {
  const weight = action.weight
  if (weight == null || !Number.isFinite(weight) || weight <= 0) return 1
  return weight
}

const ACTION_OFFSET_KEYS = [
  'angleX',
  'angleY',
  'angleZ',
  'body',
  'eyeX',
  'eyeY',
  'brow',
  'browAngSym',
  'eyeOpen',
  'irisScale',
  'armY',
  'armPos',
] as const

/** Composes only the channels owned by a finite random action clip. */
export function applyRandomActionFrame(
  target: RandomActionTarget,
  frame: Readonly<RandomActionFrame>,
  scale = 1,
): void {
  const amount = clamp(finiteOrZero(scale), 0, 1)
  target.angleX = mixChannel(target.angleX, frame.angleX, amount)
  target.angleY = mixChannel(target.angleY, frame.angleY, amount)
  target.angleZ = mixChannel(target.angleZ, frame.angleZ, amount)
  target.body = mixChannel(target.body, frame.body, amount)
  target.eyeX = mixChannel(target.eyeX, frame.eyeX, amount)
  target.eyeY = mixChannel(target.eyeY, frame.eyeY, amount)
  target.brow = mixChannel(target.brow, frame.brow, amount)
  target.browAngSym = mixChannel(target.browAngSym, frame.browAngSym, amount)
  target.eyeOpenL = mixEyeOpen(target.eyeOpenL, frame.eyeOpen * amount)
  target.eyeOpenR = mixEyeOpen(target.eyeOpenR, frame.eyeOpen * amount)
  target.irisScale = mixBoundedExpressionChannel(
    target.irisScale,
    frame.irisScale * amount,
    0.5,
    1.3,
    1,
  )
  target.armY = mixChannel(target.armY, frame.armY, amount)
  target.armPos = mixChannel(target.armPos, frame.armPos, amount)
}

function mixChannel(base: number, offset: number, scale: number): number {
  return mixBoundedExpressionChannel(base, offset * scale, -1, 1, 0)
}

function stagedEnvelope(
  progress: number,
  enterEnd: number,
  exitStart: number,
): number {
  if (progress < enterEnd) return smootherstep(progress / enterEnd)
  if (progress <= exitStart) return 1
  return 1 - smootherstep((progress - exitStart) / (1 - exitStart))
}

function copyFrame(
  target: RandomActionFrame,
  source: Readonly<RandomActionFrame>,
): void {
  for (const key of ACTION_OFFSET_KEYS) target[key] = source[key]
  target.ambientScale = source.ambientScale
}

function writeNeutral(target: RandomActionFrame): void {
  for (const key of ACTION_OFFSET_KEYS) target[key] = 0
  target.ambientScale = 1
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

function finiteTime(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? value : 0
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
