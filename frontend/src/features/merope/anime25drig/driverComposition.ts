import type { SingingGroovePose } from '../singing/singingGroove'
import type { AmbientPose } from './ambientMotion'
import type { CryMouthMotion } from './cryMotion'
import type { Anime25DDriver } from './driver'
import type { IdleBreathOffset } from './idleBreath'
import type { PerformanceExpressionOffset } from './performanceExpression'
import type { PoseGate } from './poseArbitration'
import type { OccupancyOffset } from './poseCompositor'
import type { PoseResponseController } from './poseResponse'
import type { RandomActionFrame } from './randomAction'
import type { CoSpeechExpressionOffset } from './speechExpression'
import type { AutoSpeechPose } from './speechMotion'
import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import type { ThinkingMotionPose } from './thinkingMotion'
import { sampleCryMouthMotion } from './cryMotion'
import { IDENTITY_DRIVER } from './driver'
import {
  semanticRollMotionOffset,
  semanticVerticalMotionOffset,
  speechBrowMotionOffset,
  speechHeadMotionOffset,
} from './expressiveMotionEnvelope'
import {
  applyPerformanceExpressionExtras,
  mixBoundedExpressionChannel,
  mixEyeOpen,
} from './performanceExpression'
import { POSE_KEYS } from './poseArbitration'
import {
  accumulateOccupancyOffset,
  accumulatePoseChannel,
} from './poseCompositor'
import { isContinuousPoseKey } from './poseResponse'
import {
  stepMouthForm,
  stepMouthOpen,
  stepMouthSeal,
  stepMouthShape,
} from './speechResponse'

export interface Anime25DPointerPose {
  x: number
  y: number
  inside: boolean
}

export interface Anime25DStylizedTargets {
  anger: number
  speechless: number
  maniac: number
  silly: number
  lovestruck: number
}

export interface Anime25DSecondaryMotionPose {
  angleX: number
  angleY: number
  angleZ: number
  body: number
}

export interface Anime25DBlinkState {
  activeSeconds: number
  nextAtSeconds: number
}

type RandomSource = () => number

const DRIVER_KEYS = Object.keys(IDENTITY_DRIVER) as Array<keyof Anime25DDriver>

/**
 * The authored pose, with the pointer blended over it.
 *
 * `authority` is a weight, not a switch. Every other source that contends for
 * the head crossfades — the pose gate, occupancy, the sticker mouth share all
 * ease their contribution in and out — and pointer gaze was the one exception:
 * it replaced the goal outright while the cursor was over the character and
 * dropped it the instant the cursor left, so leaving snapped the head back to
 * rest and skimming the edge of the character twitched it in and out.
 */
export function prepareAnime25DWorkingTarget(
  output: Anime25DDriver,
  authored: Readonly<Anime25DDriver>,
  pointer: Readonly<Anime25DPointerPose>,
  authority: number,
): Anime25DDriver {
  Object.assign(output, authored)
  if (!authored.mouse) return output
  const amount = clamp(Number.isFinite(authority) ? authority : 0, 0, 1)
  if (amount <= 0) return output
  output.angleX = mixPointer(authored.angleX, pointer.x * 0.9, amount)
  output.angleY = mixPointer(authored.angleY, -pointer.y * 0.7, amount)
  output.eyeX = mixPointer(authored.eyeX, pointer.x * 1.2, amount)
  output.eyeY = mixPointer(authored.eyeY, -pointer.y * 0.8, amount)
  return output
}

function mixPointer(base: number, tracked: number, amount: number): number {
  const bounded = clamp(tracked, -1, 1)
  // Full authority is the tracked value itself, not a lerp that lands one
  // float away from it — the legacy pointer math is asserted exactly.
  return amount >= 1 ? bounded : base + (bounded - base) * amount
}

/** Resolves the mutually blocked stylized-expression inputs without allocating. */
export function resolveAnime25DStylizedTargets(
  output: Anime25DStylizedTargets,
  target: Readonly<Anime25DDriver>,
  semantic: Readonly<PerformanceExpressionOffset>,
): Anime25DStylizedTargets {
  const specialEyeBlocker =
    1 -
    Math.max(
      mixBoundedExpressionChannel(target.eyeDizzy, semantic.eyeDizzy, 0, 1, 0),
      mixBoundedExpressionChannel(
        target.eyeSqueeze,
        semantic.eyeSqueeze,
        0,
        1,
        0,
      ),
      mixBoundedExpressionChannel(target.eyeCry, semantic.eyeCry, 0, 1, 0),
    )
  output.anger =
    mixBoundedExpressionChannel(target.anger, semantic.anger ?? 0, 0, 1, 0) *
    specialEyeBlocker
  output.speechless =
    mixBoundedExpressionChannel(
      target.speechless,
      semantic.speechless ?? 0,
      0,
      1,
      0,
    ) * specialEyeBlocker
  output.maniac =
    mixBoundedExpressionChannel(target.maniac, semantic.maniac ?? 0, 0, 1, 0) *
    specialEyeBlocker
  output.silly =
    mixBoundedExpressionChannel(target.silly, semantic.silly ?? 0, 0, 1, 0) *
    specialEyeBlocker
  output.lovestruck =
    mixBoundedExpressionChannel(
      target.lovestruck,
      semantic.lovestruck ?? 0,
      0,
      1,
      0,
    ) * specialEyeBlocker
  return output
}

/**
 * One arbitrated pose write for every source that contends for the same head,
 * body, eyes and brow.
 *
 * Every shared pose producer contributes a layer with the weight
 * `resolvePoseGate` derived from ownership and occupancy. Ambient motion,
 * random action, groove, thinking, breath, directed expression and co-speech
 * therefore land on the driver once instead of rewriting it in call order.
 *
 * Channels only one source writes (the random action's brow asymmetry, eye
 * openness and iris; the thinking loop's mouth corners) are uncontended, so
 * they stay on their own path — at that source's own arbitrated weight.
 */
export function applyAnime25DComposedPose(
  target: Anime25DDriver,
  gate: PoseGate,
  sources: {
    ambient: Readonly<AmbientPose>
    randomAction: Readonly<RandomActionFrame>
    groove: Readonly<SingingGroovePose>
    thinking: Readonly<ThinkingMotionPose>
    breath: Readonly<IdleBreathOffset>
    performance: Readonly<PerformanceExpressionOffset>
    stylized: Readonly<StylizedExpressionMotion>
    coSpeech: Readonly<CoSpeechExpressionOffset>
  },
  scratch: OccupancyOffset,
): void {
  for (const key of POSE_KEYS) scratch[key] = 0
  accumulateOccupancyOffset(scratch, sources.ambient, gate.ambient)
  accumulateOccupancyOffset(scratch, sources.randomAction, gate.random)
  accumulateOccupancyOffset(scratch, sources.groove, gate.groove)
  accumulateOccupancyOffset(scratch, sources.thinking, gate.thinking)
  accumulateOccupancyOffset(scratch, sources.breath, gate.ambient)
  accumulateOccupancyOffset(scratch, sources.performance, gate.performance)
  accumulateOccupancyOffset(scratch, sources.stylized, gate.stylized)
  accumulateOccupancyOffset(scratch, sources.coSpeech, gate.coSpeech)
  accumulatePoseChannel(
    scratch,
    'angleY',
    semanticVerticalMotionOffset(sources.performance.angleY),
    gate.performance,
  )
  accumulatePoseChannel(
    scratch,
    'angleZ',
    semanticRollMotionOffset(sources.performance.angleZ),
    gate.performance,
  )
  accumulatePoseChannel(
    scratch,
    'angleY',
    speechHeadMotionOffset(sources.coSpeech.angleY),
    gate.coSpeech,
  )
  accumulatePoseChannel(
    scratch,
    'brow',
    speechBrowMotionOffset(sources.coSpeech.brow),
    gate.coSpeech,
  )
  for (const key of POSE_KEYS) {
    target[key] = mixBoundedExpressionChannel(
      target[key],
      scratch[key],
      -1,
      1,
      0,
    )
  }
  applyRandomActionExpressionExtras(
    target,
    sources.randomAction,
    gate.random.expression,
  )
  applyThinkingMouth(target, sources.thinking, gate.thinking.expression)
  target.bust = mixBoundedExpressionChannel(
    target.bust,
    (sources.performance.bust ?? 0) * gate.performance.headBody,
    0,
    4,
    IDENTITY_DRIVER.bust,
  )
}

function applyRandomActionExpressionExtras(
  target: Anime25DDriver,
  frame: Readonly<RandomActionFrame>,
  amount: number,
): void {
  const weight = clamp(amount, 0, 1)
  if (weight <= 0) return
  target.browAngSym = mixBoundedExpressionChannel(
    target.browAngSym,
    frame.browAngSym * weight,
    -1,
    1,
    0,
  )
  target.eyeOpenL = mixEyeOpen(target.eyeOpenL, frame.eyeOpen * weight)
  target.eyeOpenR = mixEyeOpen(target.eyeOpenR, frame.eyeOpen * weight)
  target.irisScale = mixBoundedExpressionChannel(
    target.irisScale,
    frame.irisScale * weight,
    0.5,
    1.3,
    1,
  )
}

function applyThinkingMouth(
  target: Anime25DDriver,
  thinking: Readonly<ThinkingMotionPose>,
  amount: number,
): void {
  const weight = clamp(amount, 0, 1)
  if (weight <= 0) return
  target.mouthCY = mixBoundedExpressionChannel(
    target.mouthCY,
    thinking.mouthCY * weight,
    -1,
    1,
    0,
  )
  target.mouthCAng = mixBoundedExpressionChannel(
    target.mouthCAng,
    thinking.mouthCAng * weight,
    -1,
    1,
    0,
  )
  target.mouthScale = mixBoundedExpressionChannel(
    target.mouthScale,
    thinking.mouthScale * weight,
    0.5,
    1.5,
    1,
  )
}

export function applyAnime25DStylizedExpression(
  target: Anime25DDriver,
  semantic: Readonly<PerformanceExpressionOffset>,
  stylized: Readonly<StylizedExpressionMotion>,
  /** How much of the mouth is free of a voice, already eased by the caller. */
  vocalRest: number,
  semanticAmount = 1,
  stylizedAmount = 1,
): void {
  const semanticWeight = clamp(semanticAmount, 0, 1)
  const stylizedWeight = clamp(stylizedAmount, 0, 1)
  applyPerformanceExpressionExtras(target, semantic, semanticWeight)
  target.browAngL = mixBoundedExpressionChannel(
    target.browAngL,
    stylized.browAngL * stylizedWeight,
    -1,
    1,
    0,
  )
  target.browAngR = mixBoundedExpressionChannel(
    target.browAngR,
    stylized.browAngR * stylizedWeight,
    -1,
    1,
    0,
  )
  target.browAngSym = mixBoundedExpressionChannel(
    target.browAngSym,
    stylized.browAngSym * stylizedWeight,
    -1,
    1,
    0,
  )
  target.eyeOpenL = mixEyeOpen(
    target.eyeOpenL,
    stylized.eyeOpen * stylizedWeight,
  )
  target.eyeOpenR = mixEyeOpen(
    target.eyeOpenR,
    stylized.eyeOpen * stylizedWeight,
  )
  target.irisScale = mixBoundedExpressionChannel(
    target.irisScale,
    stylized.irisScale * stylizedWeight,
    0.5,
    1.3,
    1,
  )
  target.mouthForm = mixBoundedExpressionChannel(
    target.mouthForm,
    stylized.mouthForm * stylizedWeight,
    -1,
    1,
    0,
  )
  target.mouthOpen = Math.max(
    target.mouthOpen,
    stylized.mouthOpen * stylizedWeight,
  )
  // The silly mouth beside this one has always taken the same quantity as a
  // ratio the player eases; this took it as a boolean and stepped 0.82 of the
  // mouth within one frame every time speech started or stopped.
  const lovestruckMouthShare = 0.18 + 0.82 * clamp(vocalRest, 0, 1)
  target.mouthOpen = Math.max(
    target.mouthOpen,
    stylized.lovestruckMouthOpen * lovestruckMouthShare * stylizedWeight,
  )
  target.mouthRound = Math.max(
    target.mouthRound,
    stylized.lovestruckMouthRound * lovestruckMouthShare * stylizedWeight,
  )
  target.mouthCY = mixBoundedExpressionChannel(
    target.mouthCY,
    stylized.mouthCY * stylizedWeight,
    -1,
    1,
    0,
  )
  target.mouthCAng = mixBoundedExpressionChannel(
    target.mouthCAng,
    stylized.mouthCAng * stylizedWeight,
    -1,
    1,
    0,
  )
  target.mouthScale = mixBoundedExpressionChannel(
    target.mouthScale,
    (stylized.mouthScale +
      stylized.lovestruckMouthScale * lovestruckMouthShare) *
      stylizedWeight,
    0.5,
    1.5,
    1,
  )
}

export function applyAnime25DCryMouth(
  target: Anime25DDriver,
  currentEyeCry: number,
  timeSeconds: number,
  elapsedSeconds: number,
  output: CryMouthMotion,
): void {
  const cryResponseRate = target.eyeCry > currentEyeCry ? 6 : 4.5
  const cryAmount = clamp(
    currentEyeCry +
      (target.eyeCry - currentEyeCry) *
        (1 - Math.exp(-cryResponseRate * elapsedSeconds)),
    0,
    1,
  )
  sampleCryMouthMotion(cryAmount, timeSeconds, output)
  target.mouthOpen = Math.max(target.mouthOpen, output.mouthOpen)
  target.mouthForm = mixBoundedExpressionChannel(
    target.mouthForm,
    output.mouthForm,
    -1,
    1,
    0,
  )
  target.mouthCY = mixBoundedExpressionChannel(
    target.mouthCY,
    output.mouthCY,
    -1,
    1,
    0,
  )
  target.mouthScale = mixBoundedExpressionChannel(
    target.mouthScale,
    output.mouthScale,
    0.5,
    1.5,
    1,
  )
}

export function applyAnime25DSpeechExtras(
  target: Anime25DDriver,
  speech: Readonly<AutoSpeechPose>,
  expression: Readonly<Pick<CoSpeechExpressionOffset, 'eyeOpen'>>,
): void {
  if (
    speech.mouthOpen > 0 ||
    speech.mouthWide > 0 ||
    speech.mouthRound > 0 ||
    speech.mouthNarrow > 0 ||
    speech.mouthSeal > 0
  ) {
    target.mouthOpen = Math.max(target.mouthOpen, speech.mouthOpen)
    target.mouthWide = Math.max(target.mouthWide, speech.mouthWide)
    target.mouthRound = Math.max(target.mouthRound, speech.mouthRound)
    target.mouthNarrow = Math.max(target.mouthNarrow, speech.mouthNarrow)
    target.mouthSeal = Math.max(target.mouthSeal, speech.mouthSeal)
  }
  target.eyeOpenL = mixEyeOpen(target.eyeOpenL, expression.eyeOpen)
  target.eyeOpenR = mixEyeOpen(target.eyeOpenR, expression.eyeOpen)
}

export function applyAnime25DSillyMouthOwnership(
  target: Anime25DDriver,
  ownership: number,
): void {
  if (ownership <= 0) return
  const retained = 1 - ownership
  target.mouthOpen *= retained
  target.mouthWide *= retained
  target.mouthRound *= retained
  target.mouthNarrow *= retained
  target.mouthSeal *= retained
}

export function captureAnime25DSecondaryMotion(
  output: Anime25DSecondaryMotionPose,
  target: Readonly<Anime25DDriver>,
): void {
  output.angleX = target.angleX
  output.angleY = target.angleY
  output.angleZ = target.angleZ
  output.body = target.body
}

/** Advances the exact legacy blink curve while reusing its mutable state. */
export function stepAnime25DBlink(
  target: Anime25DDriver,
  state: Anime25DBlinkState,
  timeSeconds: number,
  elapsedSeconds: number,
  enabled: boolean,
  suppressed: boolean,
  random: RandomSource = Math.random,
): void {
  // A blink already in flight finishes, whatever happens to the reasons for
  // starting one. Both of these used to return without writing the lid at all,
  // so a sticker crossing its threshold — or automation being switched off —
  // while the eyes were shut threw them open from wherever they were.
  if (suppressed || !enabled) {
    if (suppressed) state.nextAtSeconds = timeSeconds + 1.8
    if (state.activeSeconds < 0) return
  } else {
    if (state.activeSeconds < 0 && timeSeconds > state.nextAtSeconds) {
      state.activeSeconds = 0
      state.nextAtSeconds = timeSeconds + 1.6 + random() * 3.8
      if (random() < 0.18) state.nextAtSeconds = timeSeconds + 0.28
    }
    if (state.activeSeconds < 0) return
  }
  state.activeSeconds += elapsedSeconds
  const elapsed = state.activeSeconds
  let open = 1
  if (elapsed < 0.08) {
    open = 1 - elapsed / 0.08
  } else if (elapsed < 0.42) {
    open = 0
  } else if (elapsed < 0.58) {
    open = (elapsed - 0.42) / 0.16
  } else {
    state.activeSeconds = -1
  }
  target.eyeOpenL = Math.min(target.eyeOpenL, open)
  target.eyeOpenR = Math.min(target.eyeOpenR, open)
}

/** Applies the final driver and secondary-motion response without allocations. */
export function stepAnime25DDriverResponse(
  current: Anime25DDriver,
  authored: Readonly<Anime25DDriver>,
  target: Readonly<Anime25DDriver>,
  poseResponse: PoseResponseController,
  elapsedSeconds: number,
  responseScale = 1,
): void {
  poseResponse.step(current, target, elapsedSeconds, responseScale)
  const rate = Math.min(1, elapsedSeconds * 14)
  for (const key of DRIVER_KEYS) {
    if (isContinuousPoseKey(key)) continue
    if (isAutomationFlag(key)) {
      current[key] = authored[key]
      continue
    }
    const from = current[key]
    const to = target[key]
    if (key === 'mouthOpen') {
      current.mouthOpen = stepMouthOpen(from, to, elapsedSeconds)
      continue
    }
    if (key === 'mouthForm') {
      current.mouthForm = stepMouthForm(from, to, elapsedSeconds)
      continue
    }
    if (key === 'mouthSeal') {
      current.mouthSeal = stepMouthSeal(from, to, elapsedSeconds)
      continue
    }
    if (key === 'mouthWide' || key === 'mouthRound' || key === 'mouthNarrow') {
      current[key] = stepMouthShape(from, to, elapsedSeconds)
      continue
    }
    if (key === 'eyeCry') {
      const response = to > from ? 6 : 4.5
      current.eyeCry =
        from + (to - from) * (1 - Math.exp(-response * elapsedSeconds))
      continue
    }
    if (key === 'maniac') {
      const response = to > from ? 7.2 : 4.4
      current.maniac =
        from + (to - from) * (1 - Math.exp(-response * elapsedSeconds))
      continue
    }
    if (key === 'silly') {
      const response = to > from ? 7 : 4.2
      current.silly =
        from + (to - from) * (1 - Math.exp(-response * elapsedSeconds))
      continue
    }
    if (key === 'lovestruck') {
      const response = to > from ? 6.6 : 3.8
      current.lovestruck =
        from + (to - from) * (1 - Math.exp(-response * elapsedSeconds))
      continue
    }
    current[key] = from + (to - from) * rate
  }
}

function isAutomationFlag(
  key: keyof Anime25DDriver,
): key is
  | 'idle'
  | 'blink'
  | 'rand'
  | 'thinking'
  | 'singing'
  | 'talk'
  | 'mouse'
  | 'phys' {
  return (
    key === 'idle' ||
    key === 'blink' ||
    key === 'rand' ||
    key === 'thinking' ||
    key === 'singing' ||
    key === 'talk' ||
    key === 'mouse' ||
    key === 'phys'
  )
}

export function smoothAnime25DUnit(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
