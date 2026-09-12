import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  buildMusicPlayerSnapshot,
  buildTappMediaState,
  mergeMusicContextState,
  mergeMusicPlayerEventDetail,
  pickMusicContextState,
  readLiveAudioProgress,
} from './musicPlayerState.ts'

const songA = {
  id: 'a',
  name: 'Song A',
  artist: 'A',
  cover: 'https://example.com/a.jpg',
  duration: 200,
  url: 'https://example.com/a.mp3',
}

const songB = {
  id: 'b',
  name: 'Song B',
  artist: 'B',
  cover: 'https://example.com/b.jpg',
  duration: 180,
  url: 'https://example.com/b.mp3',
}

const palette = {
  primary: '#112233',
  secondary: '#445566',
  accent: '#778899',
  light: '#aabbcc',
  dark: '#001122',
}

describe('readLiveAudioProgress', () => {
  it('returns zeros without audio', () => {
    assert.deepEqual(readLiveAudioProgress(null), {
      currentTime: 0,
      audioDuration: 0,
    })
  })

  it('reads finite currentTime and positive duration', () => {
    assert.deepEqual(
      readLiveAudioProgress({ currentTime: 12.5, duration: 200 }),
      { currentTime: 12.5, audioDuration: 200 },
    )
  })

  it('ignores NaN / non-positive duration', () => {
    assert.deepEqual(
      readLiveAudioProgress({ currentTime: Number.NaN, duration: 0 }),
      { currentTime: 0, audioDuration: 0 },
    )
  })
})

describe('buildMusicPlayerSnapshot', () => {
  const base = {
    song: songB,
    index: 3,
    colors: palette,
    isPlaying: true,
    isEnabled: true,
    volume: 0.7,
    playMode: 'loop',
    playlist: [songA, songB],
    isTempPlay: false,
    generation: 7,
  }

  it('resets progress and lyrics on song switch', () => {
    const snap = buildMusicPlayerSnapshot({
      ...base,
      resetProgress: true,
      liveCurrentTime: 99,
      liveDuration: 200,
      lyrics: [{ time: 1, text: 'old' }],
      verbatimLyrics: [{ time: 1, words: [] }],
      hasVerbatimLyrics: true,
      verbatimLyricsSource: 'netease',
      currentLyricIndex: 2,
    })
    assert.equal(snap.currentTime, 0)
    assert.equal(snap.audioDuration, 0)
    assert.deepEqual(snap.lyrics, [])
    assert.deepEqual(snap.verbatimLyrics, [])
    assert.equal(snap.hasVerbatimLyrics, false)
    assert.equal(snap.verbatimLyricsSource, '')
    assert.equal(snap.currentLyricIndex, -1)
    assert.equal(snap.generation, 7)
    assert.equal((snap.currentSong as typeof songB).id, 'b')
    assert.equal(snap.musicColor, '#112233')
  })

  it('preserves live progress and lyrics on color-only update', () => {
    const snap = buildMusicPlayerSnapshot({
      ...base,
      resetProgress: false,
      liveCurrentTime: 42.3,
      liveDuration: 200,
      lyrics: [{ time: 1, text: 'line' }],
      currentLyricIndex: 0,
    })
    assert.equal(snap.currentTime, 42.3)
    assert.equal(snap.audioDuration, 200)
    assert.equal((snap.lyrics as unknown[]).length, 1)
    assert.equal(snap.currentLyricIndex, 0)
    assert.deepEqual(snap.musicColors, palette)
  })

  it('uses default musicColor when colors null', () => {
    const snap = buildMusicPlayerSnapshot({
      ...base,
      colors: null,
      resetProgress: true,
    })
    assert.equal(snap.musicColor, '#ef4444')
    assert.equal(snap.musicColors, null)
  })
})

describe('mergeMusicPlayerEventDetail', () => {
  it('detail overrides global', () => {
    const merged = mergeMusicPlayerEventDetail(
      {
        currentSong: songA,
        musicColor: '#111',
        currentTime: 10,
      },
      {
        currentSong: songA,
        musicColor: '#222',
        musicColors: palette,
      },
    )
    assert.equal(merged.musicColor, '#222')
    assert.deepEqual(merged.musicColors, palette)
    // Keep global progress when detail omits it.
    assert.equal(merged.currentTime, 10)
  })

  it('clears stale lyrics/progress when song id changes and detail omits them', () => {
    const merged = mergeMusicPlayerEventDetail(
      {
        currentSong: songA,
        lyrics: [{ time: 0, text: 'A' }],
        currentLyricIndex: 3,
        currentTime: 55,
        audioDuration: 200,
      },
      {
        currentSong: songB,
        musicColor: '#333',
      },
    )
    assert.deepEqual(merged.lyrics, [])
    assert.equal(merged.currentLyricIndex, -1)
    assert.equal(merged.currentTime, 0)
    assert.equal(merged.audioDuration, 180)
  })

  it('respects explicit lyrics in detail on song change', () => {
    const merged = mergeMusicPlayerEventDetail(
      { currentSong: songA, lyrics: [{ time: 0, text: 'A' }] },
      {
        currentSong: songB,
        lyrics: [{ time: 0, text: 'B' }],
        currentLyricIndex: 0,
      },
    )
    assert.deepEqual(merged.lyrics, [{ time: 0, text: 'B' }])
    assert.equal(merged.currentLyricIndex, 0)
  })
})

describe('buildTappMediaState', () => {
  it('maps cover and colors for Tapp media API', () => {
    const media = buildTappMediaState({
      currentSong: songB,
      isPlaying: true,
      currentTime: 12,
      audioDuration: 180,
      volume: 0.5,
      playMode: 'shuffle',
      musicColor: palette.primary,
      musicColors: palette,
      lyrics: [],
      currentLyricIndex: -1,
      generation: 9,
      isAudioLoading: true,
    })
    assert.equal(media.currentTrack?.id, 'b')
    assert.equal(media.currentTrack?.cover, songB.cover)
    assert.equal(media.primaryColor, '#112233')
    assert.equal(media.secondaryColor, '#445566')
    assert.equal(media.position, 12)
    assert.equal(media.progress.current, 12)
    assert.equal(media.mode, 'shuffle')
    assert.equal(media.volume, 50)
    assert.equal(media.generation, 9)
    assert.equal(media.isLoading, true)
    assert.equal(media.lastError, null)
    assert.equal(media.hasThemePalette, true)
  })

  it('maps lastPlaybackError to lastError', () => {
    const media = buildTappMediaState({
      currentSong: songA,
      isPlaying: false,
      lastPlaybackError: 'playback_failed',
    })
    assert.equal(media.lastError, 'playback_failed')
  })

  it('marks hasThemePalette false without musicColors palette', () => {
    const media = buildTappMediaState({
      currentSong: songA,
      isPlaying: true,
      musicColor: '#ef4444',
    })
    assert.equal(media.hasThemePalette, false)
  })
})

describe('pickMusicContextState', () => {
  it('only picks context-owned keys (no progress clobber fields)', () => {
    const picked = pickMusicContextState({
      currentSong: songA,
      isPlaying: true,
      musicColor: '#abc',
      currentTime: 0,
      audioDuration: 0,
      musicColors: palette,
      volume: 0.2,
    })
    assert.equal(picked.currentSong, songA)
    assert.equal(picked.isPlaying, true)
    assert.equal(picked.musicColor, '#abc')
    assert.equal(Object.hasOwn(picked, 'currentTime'), false)
    assert.equal(Object.hasOwn(picked, 'musicColors'), false)
    assert.equal(Object.hasOwn(picked, 'volume'), false)
  })
})

describe('mergeMusicContextState', () => {
  it('keeps the snapshot for duplicate events and host-only progress updates', () => {
    const previous = { currentSong: songA, isPlaying: true, lyrics: [] }
    assert.equal(mergeMusicContextState(previous, previous), previous)
    assert.equal(
      mergeMusicContextState(previous, pickMusicContextState({ currentTime: 42 })),
      previous,
    )
  })

  it('applies partial changes without dropping lyrics or mutating the old snapshot', () => {
    const lyrics = [{ time: 0, text: 'A' }]
    const previous = { currentSong: songA, isPlaying: true, lyrics }
    const next = mergeMusicContextState(previous, { isPlaying: false })
    assert.notEqual(next, previous)
    assert.equal(next.isPlaying, false)
    assert.equal(previous.isPlaying, true)
    assert.equal(next.lyrics, lyrics)
    assert.equal(next.currentSong, songA)
    const cleared = mergeMusicContextState(next, { lyrics: [] })
    assert.notEqual(cleared, next)
    assert.deepEqual(cleared.lyrics, [])
  })
})
