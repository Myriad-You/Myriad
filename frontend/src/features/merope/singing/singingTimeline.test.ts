import assert from 'node:assert/strict'
import test from 'node:test'
import { compileSingingTimeline, stretchCues } from './singingTimeline'

test('stretches compiled visemes across a lyric token', () => {
  const cues = stretchCues(
    [
      { viseme: 'closed', duration: 0.05, emphasis: false },
      { viseme: 'open', duration: 0.1, emphasis: true },
    ],
    1,
    0.3,
  )
  assert.equal(cues.length, 2)
  assert.equal(cues[0].start, 1)
  assert.equal(cues[0].end, 1.1)
  assert.equal(cues[0].viseme, 'closed')
  assert.equal(cues[1].start, 1.1)
  assert.equal(cues[1].end, 1.3)
  assert.equal(cues[1].viseme, 'open')
  assert.equal(cues[1].emphasis, true)
})

test('compiles verbatim tokens onto their absolute clock', async () => {
  const cues = await compileSingingTimeline({
    verbatim: [
      {
        time: 0.5,
        duration: 0.8,
        text: 'ba',
        words: [
          { time: 0.5, duration: 0.4, text: 'ba' },
          { time: 1.2, duration: 0.3, text: '  ' },
        ],
      },
    ],
  })
  assert.ok(cues.length > 0)
  assert.equal(cues[0].start, 0.5)
  assert.ok(cues.at(-1)!.end <= 0.9 + 1e-9)
  assert.ok(
    cues.some((cue) => cue.viseme === 'closed' || cue.viseme === 'open'),
  )
})

test('fills line lyrics until the next line then rests', async () => {
  const cues = await compileSingingTimeline({
    lines: [
      { time: 1, text: 'ba' },
      { time: 3, text: 'ee' },
    ],
  })
  assert.ok(cues.length > 0)
  assert.equal(cues[0].start, 1)
  const firstLineEnd = Math.max(
    ...cues.filter((cue) => cue.start < 3).map((cue) => cue.end),
  )
  assert.ok(firstLineEnd <= 3 + 1e-9)
  assert.ok(cues.some((cue) => cue.start >= 3))
})

test('leaves an empty timeline when there are no lyrics', async () => {
  assert.deepEqual(await compileSingingTimeline({}), [])
  assert.deepEqual(
    await compileSingingTimeline({ lines: [], verbatim: [] }),
    [],
  )
})
