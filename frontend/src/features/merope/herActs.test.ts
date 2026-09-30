import assert from 'node:assert/strict'

import { beforeEach, describe, it } from 'node:test'
import { notePlaybackFailure } from '../../utils/playbackFailures'
import { forgetHerActs, herLastAct, noteNotAvailable, watchHerSong } from './herActs'

const song = { id: '186016', name: '晴天', artist: '周杰伦' }

/** A player whose state changes as time passes: `at` ms in, `then` is published. */
function player(steps: { at: number; then: () => void }[]) {
  let now = 1_000_000
  const start = now
  const w = globalThis as unknown as { window?: { __musicPlayerState?: Record<string, unknown> } }
  w.window = { __musicPlayerState: { currentSong: null, isPlaying: false } }
  const state = (patch: Record<string, unknown>) => {
    w.window!.__musicPlayerState = { ...w.window!.__musicPlayerState, ...patch }
  }
  const clock = {
    now: () => now,
    wait: async (ms: number) => {
      now += ms
      for (const step of steps.filter((step) => step.at <= now - start)) {
        step.then()
        steps.splice(steps.indexOf(step), 1)
      }
    },
  }
  return { clock, state, at: () => now }
}

describe('what came of her putting a song on', () => {
  beforeEach(() => forgetHerActs())

  it('knows when it plays', async () => {
    const { clock, state, at } = player([])
    state({ currentSong: { id: song.id }, isPlaying: false, isAudioLoading: true })
    const watching = watchHerSong('share', song, clock)
    state({ isPlaying: true, isAudioLoading: false })
    assert.equal(await watching, 'playing')
    assert.deepEqual(herLastAct(at() + 60_000), {
      act: 'share', song: '晴天', artist: '周杰伦', outcome: 'playing', secondsAgo: 60,
    })
    // Long after, it is no longer in mind.
    assert.equal(herLastAct(at() + 31 * 60_000), null)
  })

  it('knows when it would not play, and why once the player learns it', async () => {
    const steps = [
      { at: 500, then: () => notePlaybackFailure(song.id, null, clockNow()) },
      { at: 1_000, then: () => notePlaybackFailure(song.id, 'songUnavailable', clockNow()) },
    ]
    const { clock, state, at } = player(steps)
    const clockNow = () => at()
    state({ currentSong: { id: song.id }, isPlaying: false, isAudioLoading: true })
    assert.equal(await watchHerSong('share', song, clock), 'failed')
    assert.equal(herLastAct(at())?.reason, 'songUnavailable')
  })

  it('counts the player moving on before it ever played as not playing', async () => {
    const { clock, state } = player([
      { at: 500, then: () => state({ currentSong: { id: 'next' }, isPlaying: true }) },
    ])
    state({ currentSong: { id: song.id }, isAudioLoading: true })
    assert.equal(await watchHerSong('join', song, clock), 'failed')
  })

  it('says it had not started when nothing happens, and when this player cannot play it at all', async () => {
    const { clock, at } = player([])
    const quiet = { id: '4153366', name: '夜に駆ける', artist: 'YOASOBI' }
    assert.equal(await watchHerSong('share', quiet, clock), 'not_started')
    noteNotAvailable('share', quiet, at())
    assert.equal(herLastAct(at())?.outcome, 'not_available')
  })
})
