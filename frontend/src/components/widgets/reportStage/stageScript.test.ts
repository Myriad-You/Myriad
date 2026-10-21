import assert from 'node:assert/strict'
import test from 'node:test'
import { buildStageReport, cleanStageText } from './stageScript'
import { nextTickDelay, stagePlayerReducer } from './useStagePlayer'

const titles = { echo: 'Echo', insight: 'Insight', chapters: { portrait: 'Portrait' } }

test('new reports play their script and resolve fact references', () => {
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
  assert.deepEqual(report.chapters[0].beats[0].fact, { id: 'hours', label: 'Hours', value: '1200' })
  assert.equal(report.chapters[1].beats[0].fact, undefined, 'unknown fact ids are dropped, not invented')
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

test('fact labels come from the client by id, numbers are localized, media must be a site path or http', () => {
  const report = buildStageReport({
    platform: 'steam',
    facts: [
      { id: 'total_playtime_hours', label: 'Lifetime playtime, hours', value: 1234.5 },
      { id: 'top_artist', label: 'Most collected artist', value: 'Aimer', media: '/api/proxy/image?url=x' },
      { id: 'unknown_fact', label: 'Something new', value: 'v', media: 'javascript:alert(1)' },
      { id: 'protocol_relative', label: 'P', value: 'v', media: '//evil.example/a.png' },
    ],
    script: {
      chapters: [{ key: 'taste', title: '口味', beats: [
        { text: 'a', fact: 'total_playtime_hours' },
        { text: 'b', fact: 'top_artist' },
        { text: 'c', fact: 'unknown_fact' },
        { text: 'd', fact: 'protocol_relative' },
      ] }],
    },
  }, {
    ...titles,
    facts: { total_playtime_hours: '总游戏时长（小时）', top_artist: '收藏最多的歌手' },
    formatNumber: (n) => n.toLocaleString('en-US'),
  })!
  const facts = report.chapters[0].beats.map((beat) => beat.fact)
  assert.deepEqual(facts[0], { id: 'total_playtime_hours', label: '总游戏时长（小时）', value: '1,234.5' })
  assert.deepEqual(facts[1], { id: 'top_artist', label: '收藏最多的歌手', value: 'Aimer', media: '/api/proxy/image?url=x' })
  assert.deepEqual(facts[2], { id: 'unknown_fact', label: 'Something new', value: 'v' }, 'falls back to the backend label, drops unsafe media')
  assert.equal(facts[3]?.media, undefined)
})

test('cleaning keeps symbols that belong to the text and only strips paired markup', () => {
  assert.equal(cleanStageText('Wrote **C#** in `my_repo` and 5 * 3 *really* hard'), 'Wrote C# in my_repo and 5 * 3 really hard')
  assert.equal(cleanStageText('## Heading\n> quoted\n- item'), 'Heading quoted item')
  const report = buildStageReport({
    platform: 'github',
    card_visuals: {},
    facts: [{ id: 'top_language', label: 'Top language', value: 'C#' }, { id: 'top_repo', label: 'Top repo', value: 'my_repo', media: '/\\evil.example/x.png' }],
    script: { chapters: [{ key: 'taste', title: 'Taste', beats: [{ text: 'a', fact: 'top_language' }, { text: 'b', fact: 'top_repo' }] }] },
  }, titles)!
  assert.equal(report.chapters[0].beats[0].fact?.value, 'C#')
  assert.deepEqual(report.chapters[0].beats[1].fact, { id: 'top_repo', label: 'Top repo', value: 'my_repo' }, 'a backslash path is another host, not a site path')
})
