import type { ScoreBeat } from '../../../services/agent/types'
import type { SpokenUtterance } from './scoreTimeline'
import assert from 'node:assert/strict'
import test from 'node:test'
import { ScoreTimeline, timeAtText } from './scoreTimeline'

const nod = (fields: Partial<ScoreBeat>): ScoreBeat => ({ atMs: 0, offsetMs: 0, move: { kind: 'nod', amount: 0.6, count: 1, tempo: 1 }, ...fields })

function utterance(fields: Partial<SpokenUtterance>): SpokenUtterance {
  return { messageKey: 'm1', utteranceId: 'u1', text: '', startedAtMs: 1000, durationMs: 0, accents: [], ...fields }
}

test('a beat on time is placed when the score arrives, whether or not she speaks', () => {
  const timeline = new ScoreTimeline()
  timeline.set([nod({ atMs: 400 }), nod({ atMs: 1200 })], null, 5000)
  assert.deepEqual(timeline.current()?.beats.map((beat) => beat.atMs), [5400, 6200])
})

test('a beat on words lands when they are said, in order, re-timed until it is about to start', () => {
  const timeline = new ScoreTimeline()
  timeline.set([nod({ text: '其实' }), nod({ text: '其实', offsetMs: -100 }), nod({ atMs: 300 })], 'm1', 900)
  // Only the timed beat so far: the words have not been spoken.
  assert.deepEqual(timeline.current()?.beats.map((beat) => beat.id), ['1:2'])
  const text = '其实我也想过，其实不难。'
  // Predicted from text first: no accents, so at the default pace.
  timeline.noteUtterance(utterance({ text }), 1000)
  const predicted = timeline.current()!.beats
  assert.deepEqual(predicted.map((beat) => beat.id), ['1:0', '1:1', '1:2'])
  assert.equal(predicted[0]!.atMs, 1000)
  assert.equal(predicted[1]!.atMs, 1000 + 7 * 200 - 100, 'the second 其实 is its second saying')
  // The audio arrives with real accents: beats not yet started move to the real timing.
  timeline.noteUtterance(utterance({ text, durationMs: 2400, accents: [{ textOffset: 7, offsetMs: 1300, intensity: 1 }] }), 1100)
  const timed = timeline.current()!.beats
  assert.equal(timed[0]!.atMs, 1000, 'about to start: committed')
  assert.equal(timed[1]!.atMs, 1000 + 1300 - 100)
})

test('words already said when the score arrives are past, not late', () => {
  const timeline = new ScoreTimeline()
  timeline.noteUtterance(utterance({ text: '好的，我来看看。', durationMs: 1600 }), 1000)
  timeline.set([nod({ text: '好的' }), nod({ text: '看看' })], 'm1', 1800)
  const beats = timeline.current()!.beats
  assert.deepEqual(beats.map((beat) => beat.id), ['1:1'])
  // A score for another message does not take this one's words.
  const other = new ScoreTimeline()
  other.noteUtterance(utterance({ text: '好的' }), 1000)
  other.set([nod({ text: '好的' })], 'm2', 1000)
  assert.equal(other.current()?.beats.length, 0)
})

test('text position becomes time between accents, and at the measured pace past them', () => {
  const u = utterance({ text: '0123456789', durationMs: 2000, accents: [{ textOffset: 4, offsetMs: 1000, intensity: 1 }] })
  assert.equal(timeAtText(u, u.text, 2), 500)
  assert.equal(timeAtText(u, u.text, 7), 1500)
  const streaming = utterance({ text: '0123456789', accents: [{ textOffset: 4, offsetMs: 1000, intensity: 1 }] })
  assert.equal(timeAtText(streaming, streaming.text, 8), 2000)
})
