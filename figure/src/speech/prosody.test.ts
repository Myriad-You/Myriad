import assert from 'node:assert/strict'
import test from 'node:test'
import { speechProsodyTimeline } from './prosody'

test('groups adjacent emphasized visemes into future accent anchors', () => {
  const timeline = speechProsodyTimeline(
    [
      { viseme: 'open', endsAt: 0.2, emphasis: false },
      { viseme: 'wide', endsAt: 0.4, emphasis: true },
      { viseme: 'open', endsAt: 0.55, emphasis: true },
      { viseme: 'closed', endsAt: 0.8, emphasis: false },
      { viseme: 'round', endsAt: 1.2, emphasis: true },
    ],
    1.4,
  )
  assert.equal(timeline.durationMs, 1_400)
  assert.deepEqual(
    timeline.accents.map((accent) => accent.offsetMs),
    [375, 1_000],
  )
})

test('coalesces accents too close for the body to realize separately', () => {
  const timeline = speechProsodyTimeline(
    [
      { viseme: 'wide', endsAt: 0.1, emphasis: true },
      { viseme: 'rest', endsAt: 0.14, emphasis: false },
      { viseme: 'open', endsAt: 0.3, emphasis: true },
    ],
    0.4,
  )
  assert.equal(timeline.accents.length, 1)
  assert.ok(timeline.accents[0]!.intensity > 0.78)
})
