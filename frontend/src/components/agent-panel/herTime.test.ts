import type { HerDoing } from './herTime'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { herTitle, listeningAlong, nextLookMs } from './herTime'

const now = '2026-09-25T10:00:00Z'
const song: HerDoing = {
  thing: {
    kind: 'song',
    id: '186016',
    source: 'netease',
    name: '晴天',
    artist: '周杰伦',
    album: '',
    cover: '',
    durationMs: 269_000,
  },
  started: '2026-09-25T09:58:00Z',
  ends: '2026-09-25T10:02:29Z',
}

describe('her time in the panel', () => {
  it('looks again just after her thing ends, and at least every minute', () => {
    assert.equal(nextLookMs({ doing: null, now }), 60_000)
    // Lazing about ends too: she is asked again then.
    assert.equal(
      nextLookMs({
        doing: null,
        lazing: { kind: 'daydream', started: now, ends: '2026-09-25T10:00:10Z' },
        now,
      }),
      15_000,
    )
    assert.equal(
      nextLookMs({ doing: { ...song, ends: '2026-09-25T10:00:20Z' }, now }),
      25_000,
    )
    assert.equal(nextLookMs({ doing: song, now }), 60_000)
    assert.equal(
      nextLookMs({ doing: { ...song, ends: '2026-09-25T09:59:00Z' }, now }),
      5_000,
    )
  })

  it('knows when this player is on her song', () => {
    const playing = { isPlaying: true, currentSong: { id: 186016 } }
    assert.equal(listeningAlong(song, playing), true)
    assert.equal(listeningAlong(song, { ...playing, isPlaying: false }), false)
    assert.equal(
      listeningAlong(song, { isPlaying: true, currentSong: { id: '1' } }),
      false,
    )
    assert.equal(listeningAlong(null, playing), false)
    assert.equal(listeningAlong(song, null), false)
  })

  it('shows the song or note title', () => {
    assert.equal(herTitle(song), '晴天')
    assert.equal(
      herTitle({ ...song, thing: { kind: 'note', itemId: 1, title: '咖喱' } }),
      '咖喱',
    )
  })
})
