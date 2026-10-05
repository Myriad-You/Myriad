import type {
  BodyControl,
  BodyPose,
  PerformanceBaseline,
  PerformanceCue,
  ScoreBeat,
  ScoreMove,
  SpeechPhrase,
} from '../../../services/agent/types'
import contract from '../../../../../shared/merope_performance_contract.json' with { type: 'json' }

export const PERFORMANCE_BASELINE_EXPRESSIONS =
  contract.baselineExpressions as PerformanceBaseline['expression'][]
export const PERFORMANCE_POSTURES =
  contract.postures as PerformanceBaseline['posture'][]
export const PERFORMANCE_CUE_INTENTS =
  contract.cueIntents as PerformanceCue['intent'][]
export const PERFORMANCE_INTERRUPT_MODES =
  contract.interruptModes as PerformanceCue['interrupt'][]

export const BODY_CONTROLS = contract.bodyControls
export const BODY_POSE_TIMING = contract.bodyPoseTiming

/** Full target restatement, including {} as an explicit release. */
export function sanitizeBodyPose(value: unknown): BodyPose | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const raw = value as Record<string, unknown>
  if (Object.keys(raw).some(k => !['targets', 'transitionMs', 'holdMs'].includes(k))) return null
  if (!raw.targets || typeof raw.targets !== 'object' || Array.isArray(raw.targets)) return null
  const targets: BodyPose['targets'] = {}
  for (const [key, value] of Object.entries(raw.targets)) {
    if (!Object.hasOwn(BODY_CONTROLS, key) || typeof value !== 'number' || !Number.isFinite(value)) return null
    const axis = key as BodyControl
    const limits = BODY_CONTROLS[axis]
    targets[axis] = Math.max(limits.min, Math.min(limits.max, value))
  }
  const transition = raw.transitionMs ?? BODY_POSE_TIMING.transitionMs
  const hold = raw.holdMs ?? BODY_POSE_TIMING.holdMs
  if (typeof transition !== 'number' || !Number.isInteger(transition) || transition < 0
    || typeof hold !== 'number' || !Number.isInteger(hold) || hold < 0) { return null
}
  return {
    targets,
    transitionMs: Math.max(BODY_POSE_TIMING.minTransitionMs, Math.min(BODY_POSE_TIMING.maxTransitionMs, transition)),
    holdMs: Math.min(BODY_POSE_TIMING.maxHoldMs, hold),
  }
}

const PERFORMANCE_CUE_PRIORITIES = contract.cuePriorities as Record<
  PerformanceCue['intent'],
  number
>

export function performanceCuePriority(
  intent: PerformanceCue['intent'],
): number {
  return PERFORMANCE_CUE_PRIORITIES[intent]
}

export function performanceContractIsComplete(): boolean {
  return (
    new Set(PERFORMANCE_CUE_INTENTS).size === PERFORMANCE_CUE_INTENTS.length &&
    Object.keys(PERFORMANCE_CUE_PRIORITIES).length ===
      PERFORMANCE_CUE_INTENTS.length &&
    PERFORMANCE_CUE_INTENTS.every((intent) => {
      const priority = PERFORMANCE_CUE_PRIORITIES[intent]
      return Number.isInteger(priority) && priority >= 1 && priority <= 3
    })
  )
}

/** Speech phrases as the contract allows them; anything else is dropped. */
export function sanitizeSpeechPhrases(value: unknown): SpeechPhrase[] {
  if (!Array.isArray(value)) return []
  const result: SpeechPhrase[] = []
  for (const item of value.slice(0, 6)) {
    if (
      !item ||
      typeof item !== 'object' ||
      typeof item.text !== 'string' ||
      !contract.phraseIntents.includes(item.intent)
    ) {
      continue
    }
    const text = item.text.normalize('NFKC')
    const units = Iterator.from(text).reduce((n: number) => n + 1, 0)
    if (
      units < 2 ||
      units > 120 ||
      text.trim() !== text ||
      result.some((other) => other.text === text)
    ) {
      continue
    }
    result.push({ text, intent: item.intent })
  }
  return result
}

export const SCORE_TIMING = contract.scoreTiming
export const SCORE_MOVES = contract.scoreMoves as Record<ScoreMove['kind'], { controls: BodyControl[]; description: string }>

/** The score as the contract allows it, in order; a malformed beat is dropped whole. */
export function sanitizeScore(value: unknown): ScoreBeat[] {
  if (!Array.isArray(value)) return []
  const result: ScoreBeat[] = []
  for (const item of value.slice(0, SCORE_TIMING.maxBeats)) {
    const beat = sanitizeScoreBeat(item)
    if (beat) result.push(beat)
  }
  return result
}

function sanitizeScoreBeat(item: unknown): ScoreBeat | null {
  if (!item || typeof item !== 'object' || Array.isArray(item)) return null
  const raw = item as Record<string, unknown>
  let text: string | undefined
  if (raw.text !== undefined && raw.text !== null) {
    if (typeof raw.text !== 'string') return null
    text = raw.text.normalize('NFKC')
    const units = Iterator.from(text).reduce((n: number) => n + 1, 0)
    if (units === 0 || units > SCORE_TIMING.maxAnchorChars || text.trim() !== text) return null
  }
  const whole = (value: unknown, fallback: number) =>
    value === undefined || value === null ? fallback
      : typeof value === 'number' && Number.isInteger(value) ? value : null
  const atMs = whole(raw.atMs, 0)
  const offsetMs = whole(raw.offsetMs, 0)
  if (atMs === null || offsetMs === null || atMs < 0) return null
  let pose: ScoreBeat['pose']
  if (raw.pose !== undefined && raw.pose !== null) {
    const sanitized = sanitizeBodyPose(raw.pose)
    if (!sanitized) return null
    if (Object.keys(sanitized.targets).length > 0) pose = sanitized
  }
  let move: ScoreMove | undefined
  if (raw.move !== undefined && raw.move !== null) {
    const sanitized = sanitizeScoreMove(raw.move)
    if (!sanitized) return null
    move = sanitized
  }
  if (!pose && !move) return null
  return {
    ...(text !== undefined ? { text } : {}),
    atMs: text !== undefined ? 0 : Math.min(SCORE_TIMING.maxAtMs, atMs),
    offsetMs: Math.max(-SCORE_TIMING.maxOffsetMs, Math.min(SCORE_TIMING.maxOffsetMs, offsetMs)),
    ...(pose ? { pose } : {}),
    ...(move ? { move } : {}),
  }
}

function sanitizeScoreMove(value: unknown): ScoreMove | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const raw = value as Record<string, unknown>
  if (typeof raw.kind !== 'string' || !Object.hasOwn(SCORE_MOVES, raw.kind)) return null
  const word = <T extends string>(key: string, allowed: readonly string[]): T | undefined | null => {
    const entry = raw[key]
    if (entry === undefined || entry === null) return undefined
    return typeof entry === 'string' && allowed.includes(entry) ? (entry as T) : null
  }
  const side = word<NonNullable<ScoreMove['side']>>('side', contract.scoreSides)
  const direction = word<NonNullable<ScoreMove['direction']>>('direction', contract.scoreDirections)
  if (side === null || direction === null) return null
  const number = (key: string, fallback: number) => {
    const entry = raw[key]
    if (entry === undefined || entry === null) return fallback
    return typeof entry === 'number' && Number.isFinite(entry) ? entry : null
  }
  const amount = number('amount', 0.6)
  const count = number('count', 1)
  const tempo = number('tempo', 1)
  if (amount === null || count === null || tempo === null) return null
  return {
    kind: raw.kind as ScoreMove['kind'],
    ...(side ? { side } : {}),
    ...(direction ? { direction } : {}),
    amount: Math.max(0.1, Math.min(1, amount)),
    count: Math.max(1, Math.min(SCORE_TIMING.maxCount, Math.round(count))),
    tempo: Math.max(SCORE_TIMING.minTempo, Math.min(SCORE_TIMING.maxTempo, tempo)),
  }
}
