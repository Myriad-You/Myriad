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
  { name: 'postureShift', minimumDuration: 2.4, maximumDuration: 4.2 },
  { name: 'headDrift', minimumDuration: 2.6, maximumDuration: 4.4 },
  { name: 'shoulderEase', minimumDuration: 2.8, maximumDuration: 4.6 },
  {
    name: 'softBlink',
    minimumDuration: 1.6,
    maximumDuration: 2.1,
    weight: 0.8,
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
 * Automation switching back on wants life immediately; being displaced by
 * something that owns the body does not. Sharing the toggle's delay let a beat
 * manufacture idle motion rather than merely yield to it: every displacement
 * short-circuited the ordinary 2.8-7.5s gap, so the layer fired more often
 * during a conversation than during silence. Still short enough not to read
 * as a freeze — that is the other failure, and the reason this is not the
 * full idle gap either.
 */
const DISPLACED_RESUME_MIN = 0.9
const DISPLACED_RESUME_MAX = 1.8

/**
 * Cue envelope at which the idle layer stops competing for the body.
 *
 * Occupancy already thins idle motion while speaking or singing, because
 * those run alongside a gesture by design. The director's discrete beats have
 * no such term: they were only ever attenuated, never rescheduled, so a
 * `shoulderEase` could begin on the frame an `emphasize` landed and the two
 * wrote the same `body` at once.
 */
export const DIRECTED_BODY_BLOCK_LEVEL = 0.15

/**
 * Handoff between two clips, in seconds of real time.
 *
 * This used to be a fraction of the incoming clip's own duration, which made
 * the window 0.45s after a `softBlink` and 1.29s after a `shoulderEase` — the
 * length was set by whoever arrived, when what has to be shed is whatever the
 * outgoing clip left behind.
 */
export const HANDOFF_MIN = 0.28
export const HANDOFF_MAX = 0.62
/** Largest offset an idle clip authors, so residue reads as a 0-1 share. */
const HANDOFF_FULL_RESIDUE = 0.45

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
  private displaced = false
  private handoffDuration = HANDOFF_MIN

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
      // Being displaced by a live beat is not the same as automation going
      // away, and the two earn different waits on the way back.
      if (enabled) this.displaced = true
      this.available = false
      this.nextActionAt = Number.POSITIVE_INFINITY
      return this.resolveRelease(now)
    }

    if (!this.available) {
      this.available = true
      if (this.displaced) this.scheduleDisplacedAction(now)
      else this.scheduleResumeAction(now)
    }
    this.displaced = false

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

  private scheduleDisplacedAction(now: number): void {
    this.nextActionAt =
      now + this.randomRange(DISPLACED_RESUME_MIN, DISPLACED_RESUME_MAX)
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
    this.actionIntensity = this.randomRange(0.75, 1.25)
    this.nextActionAt = now + this.actionDuration + this.randomRange(2.8, 7.5)
    copyFrame(this.actionFrom, this.output)
    this.handoffDuration = idleHandoffSeconds(this.actionFrom)
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
        this.output.angleY = 0.1 * motion * intensity
        this.output.angleZ = direction * 0.14 * motion * intensity
        this.output.body = direction * 0.19 * motion * intensity
        this.output.eyeOpen = -0.035 * face * intensity
        this.output.ambientScale = 1 - 0.15 * motion
        break
      }
      case 'headDrift':
        this.output.angleX = direction * 0.15 * motion * intensity
        this.output.angleY = -0.07 * motion * intensity
        this.output.angleZ = direction * 0.16 * motion * intensity
        this.output.body = -direction * 0.12 * motion * intensity
        this.output.eyeOpen = -0.025 * face * intensity
        this.output.ambientScale = 1 - 0.18 * motion
        break
      case 'shoulderEase':
        this.output.angleX = direction * 0.1 * motion * intensity
        this.output.angleY = -0.075 * motion * intensity
        this.output.angleZ = -direction * 0.13 * motion * intensity
        this.output.body = direction * 0.22 * motion * intensity
        this.output.eyeOpen = -0.035 * face * intensity
        this.output.armY = 0.42 * gesture * intensity
        this.output.armPos = -direction * 0.18 * gesture * intensity
        this.output.ambientScale = 1 - 0.18 * motion
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
    this.blendFromPrevious(
      smootherstep((now - this.actionStartedAt) / this.handoffDuration),
    )
    return this.output
  }

  private blendFromPrevious(amount: number): void {
    if (amount >= 1) return
    // The incoming clip already owns its ease-in. Only decay the outgoing
    // residue; crossfading the new envelope again squares it and compresses
    // its visible rise into a late, sharp acceleration.
    for (const key of ACTION_OFFSET_KEYS) {
      this.output[key] += this.actionFrom[key] * (1 - amount)
    }
    this.output.ambientScale +=
      (this.actionFrom.ambientScale - 1) * (1 - amount)
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

/** A larger leftover pose needs longer to shed; a faint one is gone at once. */
export function idleHandoffSeconds(
  residue: Readonly<RandomActionFrame>,
): number {
  let peak = 0
  for (const key of ACTION_OFFSET_KEYS) {
    const amount = Math.abs(finiteOrZero(residue[key]))
    if (amount > peak) peak = amount
  }
  const share = clamp(peak / HANDOFF_FULL_RESIDUE, 0, 1)
  return mix(HANDOFF_MIN, HANDOFF_MAX, share)
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
