import type { TurnKeyformsStatus } from '../turnKeyformsApi'
import assert from 'node:assert/strict'
import test from 'node:test'
import { enhancePlanOf, estimateEnhance, ENHANCE_SECONDS as S } from './enhanceEstimate'

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
  // The head's way, plus the body's two decompositions queued on the same Space.
  const head = S.upscale + 3 * S.decompose + 4 * S.generate + 5 * S.decompose + S.fit + S.stitch
  assert.equal(estimate.remainingSeconds, head + 2 * S.decompose)
  assert.equal(estimate.fraction, 0)
})

test("the head's decomposing counts down and the body's turn reports where it is", () => {
  const estimate = estimateEnhance('figure', status({
    phase: 'head', stage: 'decomposing', done: 2, total: 5, seconds: 1200,
    body: { motion: 'turn', stage: 'decomposing', done: 1, total: 2 },
  }), 100)
  const byKey = Object.fromEntries(estimate.steps.map((step) => [step.key, step]))
  assert.equal(byKey.figureDecompose.state, 'completed')
  assert.equal(byKey.headGenerate.state, 'completed')
  assert.deepEqual([byKey.headDecompose.state, byKey.headDecompose.done, byKey.headDecompose.total], ['started', 2, 5])
  assert.equal(byKey.body.state, 'started')
  // Three of the head's left (one 100 s in), then its fit and stitch; the body's one decomposition waits in the queue.
  assert.equal(estimate.remainingSeconds, 3 * S.decompose - 100 + S.fit + S.stitch + S.decompose)
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
  const bust = estimateEnhance('bust', status({ phase: 'bust', stage: 'generating', done: 1, total: 4 }))
  assert.deepEqual(bust.steps.map((step) => step.key), ['headGenerate', 'headDecompose', 'headFit'])
  assert.equal(bust.remainingSeconds, 3 * S.generate + 5 * S.decompose + S.fit)
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
  assert.equal(head.remainingSeconds, S.upscale + 3 * S.decompose + 4 * S.generate + 5 * S.decompose + S.fit + S.stitch)
  const body = estimateEnhance('figureBody', status({ stage: 'decomposing', done: 1, total: 4 }))
  assert.deepEqual(body.steps.map((step) => [step.key, step.state]), [
    ['upscale', 'completed'], ['plainDecompose', 'started'], ['stitch', 'pending'], ['body', 'pending'],
  ])
  // Three tiles, then the body's two decompositions in the same queue, then the stitch.
  assert.equal(body.remainingSeconds, 3 * S.decompose + 2 * S.decompose + S.stitch)
})
