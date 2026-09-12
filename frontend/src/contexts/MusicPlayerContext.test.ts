import type { Song } from '../utils/musicPlayer'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { it } from 'node:test'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { bindPublishedMusicState, getNowPlaying } from './currentSong'
import { MusicPlayerProvider, useMusicLyricsSlice, useMusicPlayerControl } from './MusicPlayerContext'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', {
  paths: [require.resolve('isomorphic-dompurify')],
}))

it('shares published state inside and outside the provider without rerendering lyrics for playback changes', async () => {
  const dom = new JSDOM('<div id="root"></div>', { url: 'https://test.invalid' })
  const globals = { window: dom.window, document: dom.window.document, CustomEvent: dom.window.CustomEvent, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, key, { configurable: true, value })
  }
  const song = { id: 'shared', name: 'Shared song' } as Song
  const lyrics = [{ time: 0, text: 'line one' }]
  dom.window.__musicPlayerState = { currentSong: song, lyrics, currentLyricIndex: 0, isPlaying: false, currentTime: 42 }
  bindPublishedMusicState()
  const controls = new Map<string, ReturnType<typeof useMusicPlayerControl>>()
  let lyricRenders = 0
  function Controls({ id }: { id: string }) {
    controls.set(id, useMusicPlayerControl())
    return null
  }
  function Lyrics() {
    const state = useMusicLyricsSlice()
    lyricRenders++
    return createElement('p', null, state.lyrics[state.currentLyricIndex]?.text)
  }
  const root = createRoot(dom.window.document.getElementById('root'))
  const render = () => root.render(createElement('div', null,
    createElement(MusicPlayerProvider, { children: createElement(Controls, { id: 'inside' }) }),
    createElement(Controls, { id: 'outside' }),
    createElement(Lyrics),
  ))
  const publish = (detail: Record<string, unknown>) => {
    Object.assign(dom.window.__musicPlayerState, detail)
    dom.window.dispatchEvent(new dom.window.CustomEvent('music-player-state-change', { detail }))
  }
  try {
    await act(async () => render())
    assert.equal(controls.get('inside')?.currentSong, song)
    assert.equal(controls.get('outside')?.currentSong, song)
    assert.equal(dom.window.document.querySelector('p').textContent, 'line one')
    const beforePlayback = lyricRenders
    await act(async () => publish({ isPlaying: true }))
    for (const state of controls.values()) {
      assert.equal(state.isPlaying, true)
      assert.equal(state.lyrics, lyrics)
    }
    assert.equal(lyricRenders, beforePlayback)
    assert.deepEqual(getNowPlaying(), { song, playing: true, lyric: 'line one' })
    const beforeHostOnly = controls.get('inside')
    await act(async () => publish({ currentTime: 43 }))
    assert.equal(controls.get('inside'), beforeHostOnly)
    assert.equal(dom.window.__musicPlayerState.currentTime, 43)

    const actions: string[] = []
    for (const name of ['play-song', 'toggle-play-pause', 'stop-temp-play']) {
      dom.window.addEventListener(name, () => actions.push(name))
    }
    controls.get('inside')!.playSong(song)
    controls.get('outside')!.togglePlayPause()
    controls.get('inside')!.stopTempPlay()
    assert.deepEqual(actions, ['play-song', 'toggle-play-pause', 'stop-temp-play'])

    await act(async () => root.render(null))
    publish({ currentSong: null, lyrics: [], currentLyricIndex: -1, isPlaying: false })
    await act(async () => render())
    assert.equal(controls.get('inside')?.currentSong, null)
    assert.equal(controls.get('outside')?.isPlaying, false)
    assert.equal(dom.window.document.querySelector('p').textContent, '')
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
