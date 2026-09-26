import {
  mixBoundedExpressionChannel,
  mixEyeOpen,
} from './performanceExpression'

export type RandomActionName =
  | 'postureShift'
  | 'headDrift'
  | 'shoulderEase'
  | 'softBlink'
  | 'smile'
  | 'curiousTilt'
  | 'ponder'
  | 'hum'
  | 'yawn'

/** The small feeling an idle clip is played with; the same move reads differently under each. */
export type IdleMood = 'content' | 'relaxed' | 'curious' | 'pensive'

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
  mouthForm: number
  mouthOpen: number
  mouthRound: number
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
  /** Feelings the clip may be played with, picked afresh each time. */
  moods?: readonly IdleMood[]
}

type RandomSource = () => number

const IDLE_ACTIONS: readonly ActionDefinition[] = [
  { name: 'postureShift', minimumDuration: 2.4, maximumDuration: 4.2, moods: ['relaxed', 'content', 'pensive'] },
  { name: 'headDrift', minimumDuration: 2.6, maximumDuration: 4.4, moods: ['curious', 'content', 'pensive'] },
  { name: 'shoulderEase', minimumDuration: 2.8, maximumDuration: 4.6, moods: ['relaxed', 'content'] },
  {
    name: 'softBlink',
    minimumDuration: 1.6,
    maximumDuration: 2.1,
    weight: 0.8,
    moods: ['content', 'relaxed'],
  },
  { name: 'smile', minimumDuration: 1.8, maximumDuration: 2.6, weight: 0.9 },
  { name: 'curiousTilt', minimumDuration: 2.2, maximumDuration: 3.4, weight: 0.8 },
  { name: 'ponder', minimumDuration: 2.8, maximumDuration: 4.2, weight: 0.7 },
  { name: 'hum', minimumDuration: 3.2, maximumDuration: 4.6, weight: 0.6 },
  { name: 'yawn', minimumDuration: 2.6, maximumDuration: 3.4, weight: 0.25 },
]

type MoodFace = Pick<
  RandomActionFrame,
  'brow' | 'browAngSym' | 'eyeOpen' | 'irisScale' | 'eyeX' | 'eyeY' | 'mouthForm' | 'mouthOpen' | 'mouthRound'
>

/** Faint on purpose: a mood colours an idle move, it is not a performed expression. */
const MOOD_FACE: Readonly<Record<IdleMood, Readonly<MoodFace>>> = {
  content: { brow: 0.06, browAngSym: 0, eyeOpen: -0.07, irisScale: 0, eyeX: 0, eyeY: 0, mouthForm: 0.28, mouthOpen: 0, mouthRound: 0 },
  relaxed: { brow: -0.04, browAngSym: 0, eyeOpen: -0.2, irisScale: 0, eyeX: 0, eyeY: 0, mouthForm: 0.12, mouthOpen: 0, mouthRound: 0 },
  curious: { brow: 0.26, browAngSym: 0, eyeOpen: 0.05, irisScale: 0.05, eyeX: 0, eyeY: 0, mouthForm: 0, mouthOpen: 0.06, mouthRound: 0.14 },
  // Gaze sideways follows the clip's direction.
  pensive: { brow: 0.16, browAngSym: -0.14, eyeOpen: -0.1, irisScale: -0.03, eyeX: 0.35, eyeY: -0.28, mouthForm: -0.12, mouthOpen: 0, mouthRound: 0 },
}

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
  mouthForm: 0,
  mouthOpen: 0,
  mouthRound: 0,
  armY: 0,
  armPos: 0,
  ambientScale: 1,
}

const RELEASE_DURATION = 0.38
const RESUME_DELAY_MIN = 0.24
const RESUME_DELAY_MAX = 0.42
/** Displaced resume: longer than the toggle delay so a beat does not manufacture idle; shorter than the full idle gap. */
const DISPLACED_RESUME_MIN = 0.9
const DISPLACED_RESUME_MAX = 1.8

/** Idle stops competing for the body at this cue envelope. Director beats block rather than only attenuate. */
export const DIRECTED_BODY_BLOCK_LEVEL = 0.15

/** 两段 clip 交接窗口（秒）。长度由出段残留决定，不看出段/入段各自时长。 */
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
  private actionMood: IdleMood | null = null
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

  getActiveMood(): IdleMood | null {
    return this.activeIndex >= 0 ? this.actionMood : null
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
    const moods = action.moods ?? []
    this.actionMood = moods.length > 0
      ? moods[Math.min(moods.length - 1, Math.floor(this.randomUnit() * moods.length))]
      : null
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
      case 'smile': {
        // A closed-eye smile is a beat inside the clip, not the whole of it.
        // Plain closed lids with lifted corners: the squeezed >< eyes read as strain.
        const beat = stagedEnvelope(progress, 0.28, 0.58)
        this.output.angleY = -0.06 * motion * intensity
        this.output.angleZ = direction * 0.12 * motion * intensity
        this.output.body = direction * 0.06 * motion * intensity
        this.output.brow = 0.12 * face * intensity
        this.output.eyeOpen = -0.95 * beat
        this.output.mouthForm = 0.5 * face * intensity
        this.output.ambientScale = 1 - 0.25 * motion
        break
      }
      case 'curiousTilt':
        this.output.angleX = direction * 0.06 * motion * intensity
        this.output.angleY = 0.04 * motion * intensity
        this.output.angleZ = direction * 0.2 * motion * intensity
        this.output.body = -direction * 0.05 * motion * intensity
        this.output.brow = 0.32 * face * intensity
        this.output.eyeOpen = 0.06 * face * intensity
        this.output.irisScale = 0.06 * face * intensity
        this.output.mouthRound = 0.2 * face * intensity
        this.output.mouthOpen = 0.12 * face * intensity
        this.output.ambientScale = 1 - 0.22 * motion
        break
      case 'ponder':
        this.output.angleX = direction * 0.06 * motion * intensity
        this.output.angleY = -0.06 * motion * intensity
        this.output.angleZ = -direction * 0.14 * motion * intensity
        this.output.eyeX = direction * 0.45 * face * intensity
        this.output.eyeY = -0.32 * face * intensity
        this.output.brow = 0.2 * face * intensity
        this.output.browAngSym = -0.16 * face * intensity
        this.output.eyeOpen = -0.1 * face * intensity
        this.output.mouthForm = -0.14 * face * intensity
        this.output.ambientScale = 1 - 0.4 * motion
        break
      case 'hum': {
        // Swaying to a tune only she hears.
        const sway = Math.sin(2 * Math.PI * 1.5 * progress)
        this.output.angleZ = direction * (0.05 + 0.08 * sway) * motion * intensity
        this.output.angleX = direction * 0.04 * sway * motion * intensity
        this.output.body = direction * 0.06 * sway * motion * intensity
        this.output.brow = 0.04 * face * intensity
        this.output.eyeOpen = -0.22 * face * intensity
        this.output.mouthForm = 0.32 * face * intensity
        this.output.ambientScale = 1 - 0.3 * motion
        break
      }
      case 'yawn': {
        const open = stagedEnvelope(progress, 0.3, 0.62)
        this.output.angleY = -0.1 * motion * intensity
        this.output.angleZ = direction * 0.06 * motion * intensity
        this.output.brow = -0.08 * open
        this.output.eyeOpen = -0.6 * open
        this.output.mouthOpen = 0.7 * open
        this.output.mouthRound = 0.45 * open
        this.output.armY = 0.12 * gesture * intensity
        this.output.ambientScale = 1 - 0.35 * motion
        break
      }
    }
    if (this.actionMood) this.applyMood(MOOD_FACE[this.actionMood], face * intensity, direction)
    this.blendFromPrevious(
      smootherstep((now - this.actionStartedAt) / this.handoffDuration),
    )
    return this.output
  }

  private applyMood(mood: Readonly<MoodFace>, amount: number, direction: number): void {
    const output = this.output
    output.brow += mood.brow * amount
    output.browAngSym += mood.browAngSym * amount
    output.eyeOpen += mood.eyeOpen * amount
    output.irisScale += mood.irisScale * amount
    output.eyeX += mood.eyeX * direction * amount
    output.eyeY += mood.eyeY * amount
    output.mouthForm += mood.mouthForm * amount
    output.mouthOpen += mood.mouthOpen * amount
    output.mouthRound += mood.mouthRound * amount
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
  'mouthForm',
  'mouthOpen',
  'mouthRound',
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

/** Share of a clip's pose reached on entry; the rest is crept through while it is held. */
const HELD_CREEP = 0.16

/**
 * Enter, hold, leave. The hold is not a freeze: the pose keeps easing further
 * into itself, so it only comes to rest at the single moment it turns to leave.
 */
function stagedEnvelope(
  progress: number,
  enterEnd: number,
  exitStart: number,
): number {
  const creep = 1 - HELD_CREEP + HELD_CREEP * smootherstep(progress / exitStart)
  if (progress < enterEnd) return smootherstep(progress / enterEnd) * creep
  if (progress <= exitStart) return creep
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
