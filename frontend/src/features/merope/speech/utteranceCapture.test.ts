import assert from 'node:assert/strict'
import test from 'node:test'
import { UtteranceCapture } from './utteranceCapture'

function feed(
  capture: UtteranceCapture,
  ms: number,
  energy: number,
  size: number,
  playback = false,
) {
  const events: ReturnType<UtteranceCapture['push']>[] = []
  let remaining = Math.round((ms * capture.sampleRate) / 1000)
  while (remaining > 0) {
    const n = Math.min(size, remaining)
    events.push(capture.push(new Float32Array(n).fill(energy), playback))
    remaining -= n
  }
  return events
}

for (const size of [128, 320, 512]) {
  test(`endpoint uses milliseconds, preserves pre-roll and sentence pauses (${size} samples)`, () => {
    const capture = new UtteranceCapture(16_000)
    feed(capture, 250, 0.001, size)
    assert.equal(
      feed(capture, 240, 0.08, size).filter((e) => e.started).length,
      1,
    )
    assert.equal(
      feed(capture, 350, 0.001, size).some((e) => e.ended),
      false,
    )
    feed(capture, 240, 0.08, size)
    const ended = feed(capture, 600, 0.001, size).filter((e) => e.ended)
    assert.equal(ended.length, 1)
    const pcm = ended[0]!.utterance!.pcm
    assert.ok(
      pcm[0]![0]! < 0.01,
      'the lead-in before the opening gate is retained',
    )
    assert.ok(pcm.reduce((n, frame) => n + frame.length, 0) > 16_000)
  })
}

test('brief clicks and playback leakage do not trigger an interruption', () => {
  const capture = new UtteranceCapture(16_000)
  assert.equal(
    feed(capture, 40, 0.3, 128).some((e) => e.started),
    false,
  )
  feed(capture, 600, 0, 128)
  assert.equal(
    feed(capture, 500, 0.1, 128, true).some((e) => e.started),
    false,
  )
  assert.equal(
    feed(capture, 200, 0.2, 128, true).filter((e) => e.started).length,
    1,
  )
})

test('sustained input is bounded, reset never carries PCM into the next listener', () => {
  const capture = new UtteranceCapture(16_000)
  const ended = feed(capture, 21_000, 0.1, 128).filter((e) => e.ended)
  assert.equal(ended.length, 1)
  assert.ok(
    ended[0]!.utterance!.pcm.reduce((n, frame) => n + frame.length, 0) <=
      320_128,
  )
  capture.reset()
  assert.equal(capture.finish(), undefined)
  assert.equal(
    feed(capture, 700, 0, 128).some((e) => e.ended),
    false,
  )
})

test('a loud interruption can continue quietly after playback stops', () => {
  const capture = new UtteranceCapture(16_000)
  assert.equal(
    feed(capture, 200, 0.2, 128, true).some((event) => event.started),
    true,
  )
  assert.equal(
    feed(capture, 900, 0.025, 128).some((event) => event.ended),
    false,
  )
  assert.equal(
    feed(capture, 600, 0, 128).filter((event) => event.utterance).length,
    1,
  )
})
