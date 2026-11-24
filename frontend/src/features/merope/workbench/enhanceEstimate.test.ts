import type { TurnKeyformsStatus } from '../turnKeyformsApi'
import assert from 'node:assert/strict'
import test from 'node:test'
import { enhancePlanOf, estimateEnhance, ENHANCE_SECONDS as S } from './enhanceEstimate'

/** A bust's job from its start: the drawings, the turns' decompositions begun as they come in, the fit. */
const BUST = S.generate + S.decompose + S.fit

function status(patch: Partial<TurnKeyformsStatus>): TurnKeyformsStatus {
  return { jobId: 'j', stage: 'upscaling', done: 0, total: 1, seconds: 0, phase: 'figure', body: null, ...patch }
}

test("a figure's enhancement starts with every step ahead, the body's alongside", () => {
  const estimate = estimateEnhance('figure', status({}))
  assert.deepEqual(estimate.steps.map((step) => [step.key, step.state]), [
    ['upscale', 'started'], ['figureDecompose', 'pending'], ['headGenerate', 'pending'], ['headDecompose', 'pending'],
    ['headFit', 'pending'], ['stitch', 'pending'], ['body', 'pending'],
  ])
  assert.ok(estimate.steps.at(-1)!.parallel)
  // The head's way: the figure whole, its two tiles together, then the head as a bust (its front
  // decomposition begun with the drawings, each turn's as its drawing comes in). The body's keys
  // are made beside it and are done long before.
  assert.equal(estimate.remainingSeconds, S.upscale + 2 * S.decompose + BUST + S.stitch)
  assert.equal(estimate.fraction, 0)
})

test("the head's decomposing counts down and the body's turn reports where it is", () => {
  const estimate = estimateEnhance('figure', status({
    phase: 'head', stage: 'decomposing', done: 1, total: 5, seconds: 700,
    body: { motion: 'turn', stage: 'decomposing', done: 1, total: 2 },
  }), 20)
  const byKey = Object.fromEntries(estimate.steps.map((step) => [step.key, step]))
  assert.equal(byKey.figureDecompose.state, 'completed')
  assert.equal(byKey.headGenerate.state, 'completed')
  assert.deepEqual([byKey.headDecompose.state, byKey.headDecompose.done, byKey.headDecompose.total], ['started', 1, 5])
  assert.equal(byKey.body.state, 'started')
  // The front one is in: what is left of the four turns' decompositions, begun with their
  // drawings, 20 s on, then the fit and stitch. The body's second decomposition is about to be
  // in, on a GPU of its own.
  assert.equal(estimate.remainingSeconds, S.generate + S.decompose - S.front - 20 + S.fit + S.stitch)
  assert.ok(estimate.fraction > 0 && estimate.fraction < 1)
})

test('with the head done, what is left is the body', () => {
  const estimate = estimateEnhance('figure', status({
    phase: 'body', stage: 'fitting', seconds: 2700, body: { motion: 'turn', stage: 'fitting', done: 0, total: 1 },
  }))
  assert.ok(estimate.steps.slice(0, -1).every((step) => step.state === 'completed'))
  assert.equal(estimate.steps.at(-1)!.state, 'started')
  assert.equal(estimate.remainingSeconds, S.bodyFit)
})

test('a failed body leaves the head to finish on its own; a done job has nothing left', () => {
  const failed = estimateEnhance('figure', status({
    phase: 'head', stage: 'fitting', body: { motion: 'turn', stage: 'failed', done: 0, total: 1 },
  }))
  assert.equal(failed.steps.at(-1)!.state, 'failed')
  assert.equal(failed.remainingSeconds, S.fit + S.stitch)
  const done = estimateEnhance('figure', status({ stage: 'done', seconds: 3000 }))
  assert.equal(done.remainingSeconds, 0)
  assert.equal(done.fraction, 1)
  assert.ok(done.steps.every((step) => step.state === 'completed'))
})

test("the worn bust's plan and which plan a status follows", () => {
  const bust = estimateEnhance('bust', status({ phase: 'bust', stage: 'generating', done: 0, total: 4 }))
  assert.deepEqual(bust.steps.map((step) => step.key), ['headGenerate', 'headDecompose', 'headFit'])
  assert.equal(bust.remainingSeconds, BUST)
  // The four are drawn together: with one in, the others are about to be.
  const oneIn = estimateEnhance('bust', status({ phase: 'bust', stage: 'generating', done: 1, total: 4 }))
  assert.ok(oneIn.remainingSeconds < BUST - S.generate / 2 && oneIn.remainingSeconds > BUST - S.generate)
  const both = { head: true, body: true }
  assert.equal(enhancePlanOf(status({ phase: 'bust' }), both), 'bust')
  assert.equal(enhancePlanOf(status({ phase: 'head' }), both), 'figure')
  assert.equal(enhancePlanOf(status({ phase: 'figure' }), { head: true, body: false }), 'figureHead')
  assert.equal(enhancePlanOf(status({ phase: 'figure' }), { head: false, body: true }), 'figureBody')
  assert.equal(enhancePlanOf(status({ phase: 'refit' }), both), 'refit')
  assert.equal(enhancePlanOf(status({ phase: undefined }), both), null)
})

test("a figure keyed by its head alone has no body step; by its body alone, its tiles are all decomposed", () => {
  const head = estimateEnhance('figureHead', status({}))
  assert.ok(head.steps.every((step) => step.key !== 'body'))
  assert.equal(head.remainingSeconds, S.upscale + 2 * S.decompose + BUST + S.stitch)
  const body = estimateEnhance('figureBody', status({ stage: 'decomposing', done: 1, total: 4 }))
  assert.deepEqual(body.steps.map((step) => [step.key, step.state]), [
    ['upscale', 'completed'], ['plainDecompose', 'started'], ['stitch', 'pending'], ['body', 'pending'],
  ])
  // The waist and legs together, then the head's tile (checked as a bust's), then the stitch.
  assert.equal(body.remainingSeconds, S.decompose + S.front + S.stitch)
})

test("the bust split once has one step, the split, whose plan its status names", () => {
  const plain = estimateEnhance('plain', status({ phase: 'plain', stage: 'decomposing', done: 0, total: 1 }))
  assert.deepEqual(plain.steps.map((step) => [step.key, step.state]), [['plainDecompose', 'started']])
  assert.equal(plain.remainingSeconds, S.front)
  assert.equal(enhancePlanOf(status({ phase: 'plain' }), { head: false, body: false }), 'plain')
})

test('a turn drawn and decomposed again is one more decomposition before the fit', () => {
  const again = estimateEnhance('bust', status({ phase: 'bust', stage: 'decomposing', done: 0, total: 1 }))
  assert.equal(again.remainingSeconds, S.decompose + S.fit)
})

test("a bust's job with its steps as long as they ran on the site is estimated within a third at each stage, and never by much more than before", () => {
  // The lengths measured on the site (2026-11-24) in the order a job takes its steps: the drawings in
  // by 44 s, the front decomposition in and checked at 219 s, each turn's 207 s after its drawing
  // (27 to 44 s), then judging and the fit, 209 s. Seconds from the start, what the server reports.
  const whole = 460
  const bust = (stage: TurnKeyformsStatus['stage'], done: number, total: number) =>
    status({ phase: 'bust', stage, done, total })
  const stages: [number, TurnKeyformsStatus][] = [
    [0, bust('generating', 0, 4)],
    [44, bust('decomposing', 0, 5)],
    [219, bust('decomposing', 1, 5)],
    [251, bust('fitting', 0, 1)],
  ]
  for (const [at, reported] of stages) {
    const left = estimateEnhance('bust', { ...reported, seconds: at }).remainingSeconds
    const actual = whole - at
    assert.ok(Math.abs(left - actual) / actual <= 0.3, `${reported.stage} ${reported.done} at ${at}s: ${left} against ${actual}`)
  }
  // Between them, as the page polls: the drawings come in, the decompositions come in, the fit runs.
  const polled: [number, TurnKeyformsStatus, number][] = [
    [0, bust('generating', 0, 4), 0], [20, bust('generating', 0, 4), 20], [27, bust('generating', 1, 4), 0],
    [36, bust('generating', 3, 4), 0], [44, bust('decomposing', 0, 5), 0], [200, bust('decomposing', 0, 5), 156],
    [219, bust('decomposing', 1, 5), 0], [234, bust('decomposing', 2, 5), 0], [243, bust('decomposing', 4, 5), 0],
    [251, bust('fitting', 0, 1), 0], [370, bust('fitting', 0, 1), 119], [457, bust('fitting', 0, 1), 206],
  ]
  let before = Infinity
  for (const [at, reported, unitSeconds] of polled) {
    const left = estimateEnhance('bust', { ...reported, seconds: at }, unitSeconds).remainingSeconds
    assert.ok(left <= before + 30, `at ${at}s the time left went from ${before} to ${left}`)
    before = left
  }
})

test('a fit that goes on, a turn drawn again beside it and then fitted with, leaves a little and never less than nothing', () => {
  // The job stays at fitting 0 of 1 through the first fit, the redraw beside it and the second fit.
  let before = Infinity
  for (const unitSeconds of [0, 100, 199, 200, 285, 485, 700]) {
    const fitting = status({ phase: 'bust', stage: 'fitting', done: 0, total: 1, seconds: 251 + unitSeconds })
    const left = estimateEnhance('bust', fitting, unitSeconds).remainingSeconds
    assert.ok(left > 0 && left <= S.fit && left <= before, `${unitSeconds}s into the fit: ${left}`)
    before = left
  }
})
