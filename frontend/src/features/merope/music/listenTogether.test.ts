import type { MeropeDoingResponse, MeropeThing } from '../../../services/agent/types'
import assert from 'node:assert/strict'

import { describe, it } from 'node:test'
import { placeSong, planListenTogether } from './listenTogether'

const song: MeropeThing = {
  kind: 'song',
  id: '186016',
  source: 'netease',
  name: '晴天',
  artist: '周杰伦',
  album: '叶惠美',
  cover: 'https://example.com/c.jpg',
  durationMs: 269_000,
}

function doing(elapsedSeconds: number, thing: MeropeThing = song): MeropeDoingResponse {
  const now = Date.parse('2026-09-25T10:00:00Z')
  return {
    doing: {
      thing,
      started: new Date(now - elapsedSeconds * 1000).toISOString(),
      ends: new Date(now + 60_000).toISOString(),
    },
    now: new Date(now).toISOString(),
  }
}

const url = (id: string) => `https://audio.example/${id}`

describe('planListenTogether', () => {
  it('joins her where she is, by the server clock plus the time since asking', () => {
    const plan = planListenTogether(doing(90), null, url, 1_000, 3_000)
    assert.ok(plan)
    assert.equal(plan.offsetSeconds, 92)
    assert.equal(plan.index, null)
    assert.equal(plan.song.url, 'https://audio.example/186016')
    assert.equal(plan.song.duration, 269)
  })

  it('uses the song in this player queue when it is there', () => {
    const queue = [{ id: '1' }, { id: 186016, url: 'queued-url', name: '晴天' }]
    const plan = planListenTogether(doing(10), queue, url, 0, 0)
    assert.equal(plan?.index, 1)
    assert.equal(plan?.song.url, 'queued-url')
  })

  it('does not join a song that is nearly over, a note, or one this player cannot fetch', () => {
    assert.equal(planListenTogether(doing(265), null, url, 0, 0), null)
    assert.equal(
      planListenTogether(doing(10, { kind: 'note', itemId: 1, title: 't' }), null, url, 0, 0),
      null,
    )
    assert.equal(
      planListenTogether(doing(10, { ...song, source: 'qq' } as MeropeThing), null, url, 0, 0),
      null,
    )
    assert.equal(
      planListenTogether({ doing: null, now: new Date().toISOString() }, null, url, 0, 0),
      null,
    )
  })
})

describe('placeSong', () => {
  it('puts a song she shares on from this queue, or fetched when it is not there', () => {
    if (song.kind !== 'song') throw new Error('song')
    const queued = placeSong(song, [{ id: 186016, url: 'queued-url' }], url)
    assert.equal(queued?.index, 0)
    assert.equal(queued?.song.url, 'queued-url')
    const fetched = placeSong(song, [], url)
    assert.equal(fetched?.index, null)
    assert.equal(fetched?.song.url, 'https://audio.example/186016')
    assert.equal(placeSong({ ...song, source: 'qq' }, [], url), null)
  })
})
