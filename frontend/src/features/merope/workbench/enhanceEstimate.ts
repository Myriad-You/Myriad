import type { TurnKeyformsStatus } from '../turnKeyformsApi'

/**
 * What an enhancement job does, step by step, and about how long is left.
 * The server reports a stage and which part of the work it belongs to
 * (`phase`: a figure's decomposing and its head's are both `decomposing`);
 * the steps and their usual lengths are known here. Drawings are made
 * together, and so are decompositions that need nothing of each other: a
 * step's units are not each as long as the step.
 */
export type EnhancePlan = 'bust' | 'figure' | 'figureHead' | 'figureBody' | 'refit' | 'plain'

export type EnhanceStepKey =
  | 'upscale'
  | 'figureDecompose'
  | 'plainDecompose'
  | 'headGenerate'
  | 'headDecompose'
  | 'headFit'
  | 'stitch'
  | 'body'
  | 'refit'

export type EnhanceStepState = 'pending' | 'started' | 'completed' | 'failed'

export interface EnhanceStep {
  key: EnhanceStepKey
  state: EnhanceStepState
  /** Units done and in all (drawings, decompositions), where the step counts them. */
  done: number
  total: number
  /** Made alongside the others (a figure's body keys). */
  parallel: boolean
}

export interface EnhanceEstimate {
  steps: EnhanceStep[]
  elapsedSeconds: number
  /** About how long is left, seconds. */
  remainingSeconds: number
  /** Share done, 0…1, by expected time. */
  fraction: number
}

/**
 * Usual lengths, seconds, measured on a bust's job on the site (2026-11-24):
 * the four drawings came within 27-44 s of each other's start; the front
 * decomposition, started with them, was in and checked 219 s after the job
 * began; a turned decomposition, four sent together, took 207 s; the fit
 * 200 s (the Space reports 189-194 of its own). Each turned drawing is
 * decomposed from when it is in, beside the front decomposition: the turns'
 * are in about a decomposition's length after the drawings, a little after
 * the front one. The drawn turns are checked against the front decomposition
 * beside that; the check's own length is not in these figures, and neither
 * is a turn it sends back, which is drawn and decomposed again. The body's
 * fit took 22 s on a Mac and was not timed on the Space: a cautious figure.
 * Upscaling and stitching are as they were (2026-10).
 */
export const ENHANCE_SECONDS = {
  upscale: 60,
  /** The four turned drawings, made together (or the body's two). */
  generate: 45,
  /** The front decomposition with its upload and check, from when the drawings start. */
  front: 220,
  /** One decomposition, or several sent together. */
  decompose: 210,
  fit: 200,
  stitch: 60,
  bodyFit: 60,
} as const

/** Units of a step that the server counts, made one after another or all together. */
interface Run {
  units: number
  /** Seconds each takes when made one after another; all of them when made together. */
  seconds: number
  together?: boolean
}

interface StepModel {
  key: EnhanceStepKey
  runs: Run[]
  /** Seconds a unit takes when the server counts others than `runs` (a turn drawn and decomposed again). */
  again?: number
}

/** With the first of some made together in, the others are about to be: the share of their time still ahead. */
const TOGETHER_TAIL = 0.2

const S = ENHANCE_SECONDS
const one = (seconds: number): Run[] => [{ units: 1, seconds }]
const tiles: Run = { units: 2, seconds: S.decompose, together: true }

const HEAD: StepModel[] = [
  { key: 'headGenerate', runs: [{ units: 4, seconds: S.generate, together: true }] },
  {
    key: 'headDecompose',
    runs: [
      // Begun with the drawings: what is left of it once they are in.
      { units: 1, seconds: S.front - S.generate },
      // Each begun as its drawing came in: what is left of them once the front one is in.
      { units: 4, seconds: S.generate + S.decompose - S.front, together: true },
    ],
    again: S.decompose,
  },
  { key: 'headFit', runs: one(S.fit) },
]
const UPSCALE: StepModel = { key: 'upscale', runs: one(S.upscale) }
const STITCH: StepModel = { key: 'stitch', runs: one(S.stitch) }
/** The figure whole, then its waist and legs together; its head's tile is the head's front decomposition. */
const FIGURE_DECOMPOSE: StepModel = { key: 'figureDecompose', runs: [...one(S.decompose), tiles] }

/** The head's way of each plan; a figure's body keys run beside it (`withBody`). */
const PLANS: Record<EnhancePlan, StepModel[]> = {
  bust: HEAD,
  figure: [UPSCALE, FIGURE_DECOMPOSE, ...HEAD, STITCH],
  figureHead: [UPSCALE, FIGURE_DECOMPOSE, ...HEAD, STITCH],
  // The head not keyed, its tile is decomposed (and checked) after the others.
  figureBody: [UPSCALE, { key: 'plainDecompose', runs: [...one(S.decompose), tiles, ...one(S.front)] }, STITCH],
  refit: [{ key: 'refit', runs: one(S.fit) }],
  // The bust split once, for its redrawn expressions.
  plain: [{ key: 'plainDecompose', runs: one(S.front) }],
}

function unitsOf(model: StepModel): number {
  return model.runs.reduce((units, run) => units + run.units, 0)
}

/** How long a step takes from its start. */
function secondsOf(model: StepModel): number {
  return model.runs.reduce((seconds, run) => seconds + (run.together ? run.seconds : run.units * run.seconds), 0)
}

/** How long is left of a step with `done` of its units in, `unitSeconds` after the last one came in. */
function secondsLeft(model: StepModel, done: number, total: number, unitSeconds: number): number {
  if (total !== unitsOf(model)) {
    // Not the units the step is made of: one of them made again.
    const unit = model.again ?? model.runs[model.runs.length - 1].seconds
    return Math.max(Math.max(0, total - done) * unit - unitSeconds, unit * 0.1)
  }
  let left = 0
  let first = 0
  let reached = false
  for (const run of model.runs) {
    const here = Math.min(Math.max(done - first, 0), run.units)
    first += run.units
    if (here >= run.units) continue
    if (reached) {
      left += run.together ? run.seconds : run.units * run.seconds
      continue
    }
    // The run being made now.
    reached = true
    const ahead = !run.together
      ? (run.units - here) * run.seconds
      : here === 0
        ? run.seconds
        : run.seconds * TOGETHER_TAIL * (run.units - here) / run.units
    left += Math.max(ahead - unitSeconds, run.seconds * 0.1)
  }
  return left
}

function withBody(plan: EnhancePlan): boolean {
  return plan === 'figure' || plan === 'figureBody'
}

/** The step a status is in, for a plan; null once done (or not one of its stages). */
function currentStep(plan: EnhancePlan, status: TurnKeyformsStatus): EnhanceStepKey | null {
  const { phase, stage } = status
  if (stage === 'done' || stage === 'failed') return null
  if (plan === 'refit') return 'refit'
  if (plan === 'plain') return 'plainDecompose'
  if (plan === 'bust') {
    return stage === 'generating' ? 'headGenerate' : stage === 'decomposing' ? 'headDecompose' : 'headFit'
  }
  if (phase === 'head') {
    return stage === 'generating' ? 'headGenerate' : stage === 'decomposing' ? 'headDecompose' : 'headFit'
  }
  if (phase === 'body') return 'body'
  if (stage === 'upscaling') return 'upscale'
  if (stage === 'stitching') return 'stitch'
  return plan === 'figureBody' ? 'plainDecompose' : 'figureDecompose'
}

/** The body's turn: its two drawings made together, its two decompositions sent together, one fit. */
const BODY_MOTIONS = ['turn'] as const

/** How long is left of the body's keys from where they stand: they are made beside the head's work. */
function bodySecondsLeft(status: TurnKeyformsStatus): number {
  const body = status.body
  if (!body) return S.generate + S.decompose + S.bodyFit
  // Two made together: all of their time with neither in, a little with one in.
  const together = (seconds: number) =>
    body.done === 0 ? seconds : seconds * TOGETHER_TAIL * Math.max(0, body.total - body.done) / Math.max(body.total, 1)
  if (body.stage === 'generating') return together(S.generate) + S.decompose + S.bodyFit
  if (body.stage === 'decomposing') return together(S.decompose) + S.bodyFit
  if (body.stage === 'fitting') return S.bodyFit
  return 0
}

function bodyState(status: TurnKeyformsStatus, headDone: boolean): EnhanceStepState {
  const body = status.body
  if (status.stage === 'done') return 'completed'
  if (body?.stage === 'failed') return 'failed'
  if (body && body.motion === BODY_MOTIONS[BODY_MOTIONS.length - 1] && body.stage === 'done') return 'completed'
  if (body || headDone) return 'started'
  return 'pending'
}

/**
 * The steps of `plan` as `status` has them, and about how long is left.
 * `unitSeconds`: how long the current unit (drawing, decomposition) has run,
 * as far as the page has seen.
 */
export function estimateEnhance(
  plan: EnhancePlan,
  status: TurnKeyformsStatus,
  unitSeconds = 0,
): EnhanceEstimate {
  const models = PLANS[plan]
  const current = currentStep(plan, status)
  const finished = status.stage === 'done'
  const at = current ? models.findIndex((model) => model.key === current) : finished ? models.length : -1
  // Waiting on the body: every step of the head's way is done.
  const headIndex = current === 'body' ? models.length : at
  const steps: EnhanceStep[] = models.map((model, index) => {
    const here = index === headIndex
    const units = unitsOf(model)
    return {
      key: model.key,
      state: status.stage === 'failed' && here ? 'failed' : index < headIndex ? 'completed' : here ? 'started' : 'pending',
      done: here ? Math.min(status.done, status.total) : index < headIndex ? units : 0,
      total: here ? Math.max(status.total, 1) : units,
      parallel: false,
    }
  })
  let headSeconds = 0
  models.forEach((model, index) => {
    if (index < headIndex) return
    headSeconds += index === headIndex
      ? secondsLeft(model, status.done, status.total, unitSeconds)
      : secondsOf(model)
  })
  let bodySeconds = 0
  if (withBody(plan)) {
    // Made beside the head's work, on GPUs of its own: whichever ends later is what is left.
    bodySeconds = bodySecondsLeft(status)
    const body = status.body
    steps.push({
      key: 'body',
      state: bodyState(status, headIndex >= models.length),
      done: body ? Math.min(body.done, body.total) : 0,
      total: body ? Math.max(body.total, 1) : 1,
      parallel: true,
    })
  }
  const remainingSeconds = finished ? 0 : Math.round(Math.max(headSeconds, bodySeconds))
  const elapsedSeconds = Math.max(0, status.seconds)
  const whole = elapsedSeconds + remainingSeconds
  return {
    steps,
    elapsedSeconds,
    remainingSeconds,
    fraction: finished ? 1 : whole > 0 ? Math.min(0.99, elapsedSeconds / whole) : 0,
  }
}

/** The plan a running job follows, from where it is reported and the parts the page asked a figure for. */
export function enhancePlanOf(status: TurnKeyformsStatus, parts: { head: boolean; body: boolean }): EnhancePlan | null {
  if (!status.phase) return null
  if (status.phase === 'refit') return 'refit'
  if (status.phase === 'load') return null
  if (status.phase === 'plain') return 'plain'
  if (status.phase === 'bust') return 'bust'
  if (parts.head && parts.body) return 'figure'
  if (parts.head) return 'figureHead'
  return parts.body ? 'figureBody' : null
}
