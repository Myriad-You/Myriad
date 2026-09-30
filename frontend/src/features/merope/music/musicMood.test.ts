import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  MUSIC_MOOD_REPORT_SECONDS,
  MusicListeningAccumulator,
} from './musicMood'

function playing(currentTime: number) {
  return {
    songId: 'track-1',
    currentTime,
    playing: true,
    audible: true,
    loading: false,
  }
}

describe('MusicListeningAccumulator', () => {
  it('qualifies accumulated audible playback across ordinary progress ticks', () => {
    const accumulator = new MusicListeningAccumulator()
    accumulator.observe(playing(0), 0)
    for (let second = 1; second <= MUSIC_MOOD_REPORT_SECONDS; second += 1) {
      accumulator.observe(playing(second), second * 1000)
    }
    assert.equal(accumulator.qualified(), true)
    assert.equal(accumulator.takeReport(), MUSIC_MOOD_REPORT_SECONDS)
    assert.equal(accumulator.qualified(), false)
  })

  it('does not count paused, muted, loading or cross-track progress', () => {
    const accumulator = new MusicListeningAccumulator()
    accumulator.observe(playing(0), 0)
    accumulator.observe({ ...playing(20), playing: false }, 20_000)
    accumulator.observe({ ...playing(40), audible: false }, 40_000)
    accumulator.observe({ ...playing(60), loading: true }, 60_000)
    accumulator.observe({ ...playing(80), songId: 'track-2' }, 80_000)
    assert.equal(accumulator.secondsForTest(), 0)
  })

  it('caps seek jumps by plausible elapsed playback time', () => {
    const accumulator = new MusicListeningAccumulator()
    accumulator.observe(playing(5), 10_000)
    accumulator.observe(playing(305), 11_000)
    assert.ok(accumulator.secondsForTest() <= 2.25)
    assert.ok(accumulator.secondsForTest() > 0)
  })
})
