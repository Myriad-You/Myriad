import assert from 'node:assert/strict'
import test from 'node:test'
import { buildStageReport, cleanStageText } from './stageScript'
import { nextTickDelay, stagePlayerReducer } from './useStagePlayer'

const titles = { echo: 'Echo', insight: 'Insight', chapters: { portrait: 'Portrait' } }

test('new reports play their script; fact references are not read', () => {
  const report = buildStageReport({
    platform: 'steam',
    summary: 'ignored',
    insights: ['ignored'],
    card_visuals: { a: 1 },
    facts: [{ id: 'hours', label: 'Hours', value: 1200 }],
    script: {
      headline: 'A *long* year',
      chapters: [
        { key: 'portrait', title: '', beats: [{ text: 'You **played** a lot.', fact: 'hours' }, { text: '  ' }] },
        { key: 'taste', title: 'Taste', beats: [{ text: 'Mostly RPGs.', fact: 'missing' }] },
      ],
    },
    style_stale: true,
  }, titles)!
  assert.equal(report.headline, 'A long year')
  assert.deepEqual(report.chapters.map((c) => [c.key, c.title, c.beats.length]), [['portrait', 'Portrait', 1], ['taste', 'Taste', 1]])
  assert.deepEqual(report.chapters[0].beats[0], { text: 'You played a lot.', holdMs: report.chapters[0].beats[0].holdMs }, 'the stage shows no facts')
  assert.equal(report.styleStale, true)
})

test('old reports fall back to an echo act and an insight act, split at sentence ends only', () => {
  const report = buildStageReport({
    platform: 'github',
    summary: '写了很多代码，提交不少。周末也在写！',
    insights: ['Mostly Rust. Some TS'],
  }, titles)!
  assert.deepEqual(report.chapters.map((c) => c.beats.map((b) => b.text)), [
    ['写了很多代码，提交不少。', '周末也在写！'],
    ['Mostly Rust.', 'Some TS'],
  ])
})

test('nothing to play gives no report', () => {
  assert.equal(buildStageReport({ platform: 'x', summary: '', insights: [] }, titles), null)
  assert.equal(buildStageReport({ summary: 'no platform' }, titles), null)
})

test('the player walks beats, then acts, then ends; pause keeps the place', () => {
  const chapters = buildStageReport({ platform: 'x', summary: 'One. Two.', insights: ['Three.'] }, titles)!.chapters
  let state = { chapter: 0, shown: 0, playing: true, ended: false }
  assert.equal(nextTickDelay(state, chapters), 500)
  state = stagePlayerReducer(state, { type: 'tick', chapters })
  state = stagePlayerReducer(state, { type: 'toggle' })
  assert.deepEqual(state, { chapter: 0, shown: 1, playing: false, ended: false }, 'paused on the same line')
  state = stagePlayerReducer(state, { type: 'toggle' })
  state = stagePlayerReducer(state, { type: 'tick', chapters })
  assert.ok(nextTickDelay(state, chapters) > chapters[0].beats[1].holdMs, 'last line of an act lingers')
  state = stagePlayerReducer(state, { type: 'tick', chapters })
  assert.deepEqual([state.chapter, state.shown], [1, 0])
  state = stagePlayerReducer(stagePlayerReducer(state, { type: 'tick', chapters }), { type: 'tick', chapters })
  assert.equal(state.ended, true)
  state = stagePlayerReducer(state, { type: 'toggle' })
  assert.deepEqual(state, { chapter: 0, shown: 0, playing: true, ended: false }, 'play after the end replays')
  assert.equal(stagePlayerReducer(state, { type: 'jump', chapter: 9, chapters }).chapter, 1)
})

test('cleaning keeps symbols that belong to the text and only strips paired markup', () => {
  assert.equal(cleanStageText('Wrote **C#** in `my_repo` and 5 * 3 *really* hard'), 'Wrote C# in my_repo and 5 * 3 really hard')
  assert.equal(cleanStageText('## Heading\n> quoted\n- item'), 'Heading quoted item')
})
