import type { WatchProgressLabels } from './libraryWatchProgress'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  formatWatchProgressText,
  formatWatchStatusLabel,
  getWatchProgress,
  parseProgressString,
} from './libraryWatchProgress'

const labels: WatchProgressLabels = {
  progressEp: 'EP {current}/{total}',
  progressEpOnly: 'EP {current}',
  progressCh: 'Ch {current}/{total}',
  progressChOnly: 'Ch {current}',
  progressVol: 'Vol {current}/{total}',
  progressVolOnly: 'Vol {current}',
  progressJoin: ' · ',
  statusDoing: 'Watching',
  statusDone: 'Completed',
  statusWish: 'Plan to watch',
  statusOnHold: 'On hold',
  statusDropped: 'Dropped',
}

describe('parseProgressString', () => {
  it('parses n/m', () => {
    assert.deepEqual(parseProgressString('5/12'), { current: 5, total: 12 })
  })
  it('parses n only', () => {
    assert.deepEqual(parseProgressString('5'), { current: 5, total: null })
  })
  it('parses n/? as unknown total', () => {
    assert.deepEqual(parseProgressString('5/?'), { current: 5, total: null })
  })
  it('rejects empty', () => {
    assert.equal(parseProgressString(''), null)
    assert.equal(parseProgressString(null), null)
  })
})

describe('getWatchProgress anime/MAL', () => {
  it('uses metadata.progress string', () => {
    const p = getWatchProgress('anime', { progress: '8/24', status: 'watching' })
    assert.ok(p)
    assert.equal(p!.primary.current, 8)
    assert.equal(p!.primary.total, 24)
    assert.equal(p!.percent, 33)
    assert.equal(p!.status, 'doing')
  })

  it('enriches total from node.num_episodes when progress is current-only', () => {
    const p = getWatchProgress('anime', {
      progress: '5',
      node: { num_episodes: 12 },
    })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 5, total: 12 })
  })
})

describe('getWatchProgress anime/Bangumi', () => {
  it('reads ep_status + subject.eps', () => {
    const p = getWatchProgress('anime', {
      ep_status: 5,
      type: 3,
      subject: { eps: 12 },
    })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 5, total: 12 })
    assert.equal(p!.status, 'doing')
    assert.equal(p!.percent, 42)
  })

  it('shows watched only when total missing', () => {
    const p = getWatchProgress('anime', { ep_status: 3, type: 3 })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 3, total: null })
    assert.equal(p!.percent, null)
  })

  it('hides zero progress with no total', () => {
    assert.equal(
      getWatchProgress('anime', { ep_status: 0, type: 1 }),
      null,
    )
  })

  it('shows full progress when completed', () => {
    const p = getWatchProgress('anime', {
      ep_status: 12,
      type: 2,
      subject: { eps: 12 },
    })
    assert.ok(p)
    assert.equal(p!.percent, 100)
    assert.equal(p!.status, 'done')
  })

  it('fills empty ep_status to total when Bangumi marked done', () => {
    const p = getWatchProgress('anime', {
      ep_status: 0,
      type: 2,
      subject: { eps: 24 },
    })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 24, total: 24 })
    assert.equal(p!.percent, 100)
    assert.equal(p!.status, 'done')
  })

  it('fills missing progress to total when Bangumi marked done', () => {
    const p = getWatchProgress('anime', {
      type: 2,
      subject: { eps: 13 },
    })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 13, total: 13 })
    assert.equal(p!.percent, 100)
  })

  it('fills zero watched to total when MAL completed', () => {
    const p = getWatchProgress('anime', {
      progress: '0/12',
      status: 'completed',
      node: { num_episodes: 12 },
    })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 12, total: 12 })
    assert.equal(p!.percent, 100)
    assert.equal(p!.status, 'done')
  })

  it('fills partial logged progress to total when completed', () => {
    const p = getWatchProgress('anime', {
      ep_status: 3,
      type: 2,
      subject: { eps: 12 },
    })
    assert.ok(p)
    assert.deepEqual(p!.primary, { current: 12, total: 12 })
    assert.equal(p!.percent, 100)
  })
})

describe('getWatchProgress book', () => {
  it('formats chapters and volumes for Bangumi', () => {
    const p = getWatchProgress('book', {
      ep_status: 10,
      vol_status: 2,
      type: 3,
      subject: { eps: 50, volumes: 5 },
    })
    assert.ok(p)
    assert.deepEqual(p!.chapters, { current: 10, total: 50 })
    assert.deepEqual(p!.volumes, { current: 2, total: 5 })
    assert.equal(
      formatWatchProgressText(p!, labels),
      'Ch 10/50 · Vol 2/5',
    )
  })

  it('uses MAL flattened ep_status/vol_status', () => {
    const p = getWatchProgress('book', {
      ep_status: 20,
      vol_status: 1,
      status: 'reading',
      node: { num_chapters: 100, num_volumes: 10 },
    })
    assert.ok(p)
    assert.equal(p!.status, 'doing')
    assert.deepEqual(p!.chapters, { current: 20, total: 100 })
    assert.deepEqual(p!.volumes, { current: 1, total: 10 })
  })

  it('does not show 0/0 book noise', () => {
    assert.equal(
      getWatchProgress('book', { ep_status: 0, vol_status: 0 }),
      null,
    )
  })

  it('fills empty chapter progress when book marked done', () => {
    const p = getWatchProgress('book', {
      ep_status: 0,
      vol_status: 0,
      type: 2,
      subject: { eps: 40, volumes: 4 },
    })
    assert.ok(p)
    assert.deepEqual(p!.chapters, { current: 40, total: 40 })
    assert.deepEqual(p!.volumes, { current: 4, total: 4 })
    assert.equal(p!.percent, 100)
    assert.equal(p!.status, 'done')
  })

  it('fills zero chapters when MAL manga completed', () => {
    const p = getWatchProgress('book', {
      status: 'completed',
      list_status: { num_chapters_read: 0, num_volumes_read: 0 },
      node: { num_chapters: 100, num_volumes: 10 },
    })
    assert.ok(p)
    assert.deepEqual(p!.chapters, { current: 100, total: 100 })
    assert.deepEqual(p!.volumes, { current: 10, total: 10 })
  })

  it('ignores game/music', () => {
    assert.equal(getWatchProgress('game', { playtime_forever: 100 }), null)
    assert.equal(getWatchProgress('music', { artist: 'x' }), null)
  })
})

describe('formatWatchStatusLabel', () => {
  it('defaults to doing only', () => {
    assert.equal(
      formatWatchStatusLabel('doing', labels),
      'Watching',
    )
    assert.equal(formatWatchStatusLabel('done', labels), null)
  })
})

describe('formatWatchProgressText anime', () => {
  it('uses EP template', () => {
    const p = getWatchProgress('anime', { progress: '5/12' })!
    assert.equal(formatWatchProgressText(p, labels), 'EP 5/12')
  })
})
