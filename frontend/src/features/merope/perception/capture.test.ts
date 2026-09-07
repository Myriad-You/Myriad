import type { Song } from '../../../utils/musicPlayer'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  agentMusicStatus,
  applyPublishedMusicState,
  getCurrentSong,
  getNowPlaying,
  setCurrentSongSnapshot,
} from '../../../contexts/currentSong'
import {
  replaceMusicTrackSource,
  replaceSurfaceSource,
} from './consentedSources'
import { KINDS, MAX_PERCEPTION_ITEMS, perceptionRegistry } from './registry'
import { setForegroundSurface } from './surface'

const TRACK_TTL_MS = 2_000
const SURFACE_TTL_MS = 4_000

function sampleSong(name: string, artist: string): Song {
  return {
    id: 'track-1',
    name,
    artist,
    album: 'Demo',
    cover: 'https://example.test/cover.jpg',
    url: 'https://example.test/secret.mp3',
    duration: 180,
    source: 'netease',
  }
}

function bySource(sourceId: string) {
  return perceptionRegistry.active().find((item) => item.sourceId === sourceId)
}

function captureConsented(pageConsent: boolean): void {
  const now = Date.now()
  replaceSurfaceSource({ now, ttlMs: SURFACE_TTL_MS })
  replaceMusicTrackSource({ now, pageConsent, ttlMs: TRACK_TTL_MS })
}

test.describe('perception capture', { concurrency: false }, () => {
  test('music_track stays behind page consent', () => {
    applyPublishedMusicState({ currentSong: sampleSong('Night', 'Lantern') })
    setForegroundSurface('none')
    captureConsented(false)
    assert.equal(bySource('music_track'), undefined)
  })

  test('player publish path names the track and omits url', () => {
    setForegroundSurface('none')
    setCurrentSongSnapshot(null)
    applyPublishedMusicState({
      currentSong: sampleSong('Night', 'Lantern'),
      isPlaying: true,
    })
    assert.equal(getCurrentSong()?.name, 'Night')
    captureConsented(true)
    const first = bySource('music_track')
    assert.ok(first)
    assert.equal(first.summary, 'Night — Lantern')
    assert.equal(first.safeFacts.title, 'Night')
    assert.equal(first.safeFacts.artist, 'Lantern')
    assert.equal(first.safeFacts.album, 'Demo')
    assert.equal(first.safeFacts.source, 'netease')
    assert.equal(first.safeFacts.playing, true)
    assert.equal('url' in first.safeFacts, false)
    assert.equal('cover' in first.safeFacts, false)
    assert.equal('id' in first.safeFacts, false)
    const firstRevision = first.revision

    applyPublishedMusicState({
      currentSong: sampleSong('Dawn', 'Lantern'),
      isPlaying: true,
    })
    captureConsented(true)
    const second = bySource('music_track')
    assert.ok(second)
    assert.equal(second.summary, 'Dawn — Lantern')
    assert.ok(second.revision > firstRevision)
    assert.equal('url' in second.safeFacts, false)
  })

  test('partial play/pause publish does not clear the track', () => {
    applyPublishedMusicState({ currentSong: sampleSong('Night', 'Lantern') })
    applyPublishedMusicState({ isPlaying: false })
    assert.equal(getCurrentSong()?.name, 'Night')
    assert.equal(getNowPlaying().playing, false)
    captureConsented(true)
    const paused = bySource('music_track')
    assert.ok(paused)
    assert.equal(paused.summary, '已暂停 Night — Lantern')
    assert.equal(paused.safeFacts.playing, false)
  })

  test('current lyric line is a fact, full lyrics are not', () => {
    applyPublishedMusicState({
      currentSong: sampleSong('Night', 'Lantern'),
      isPlaying: true,
      lyrics: [
        { time: 0, text: 'first line' },
        { time: 12, text: 'harbour light' },
      ],
      currentLyricIndex: 1,
    })
    captureConsented(true)
    const row = bySource('music_track')
    assert.ok(row)
    assert.equal(row.safeFacts.lyric, 'harbour light')
    assert.ok(String(row.summary).includes('harbour light'))
    assert.equal('lyrics' in row.safeFacts, false)
  })

  test('closed overlay reports surface none', () => {
    setForegroundSurface('none')
    captureConsented(false)
    const row = bySource('surface')
    assert.ok(row)
    assert.equal(row.kind, 'surface')
    assert.equal(row.safeFacts.surface, 'none')
    assert.equal(row.privacy, 'local')
  })
})

test('provider event path writes the current-song snapshot', () => {
  const source = readFileSync(
    new URL('../../../contexts/MusicPlayerContext.tsx', import.meta.url),
    'utf8',
  )
  const provider = source
    .split('export function MusicPlayerProvider')[1]
    ?.split('export function useMusicPlayerControl')[0]
  assert.ok(provider)
  assert.match(provider, /applyPublishedMusicState\(/)
  assert.match(provider, /music-player-state-change/)
  const boot = source.split('初始化：监听事件并更新全局状态')[1]
  assert.ok(boot)
  assert.match(boot, /applyPublishedMusicState\(/)
  assert.match(boot, /bindPublishedMusicState\(/)
  assert.match(boot, /attachMusicEventListener\(/)
})

test('capture wires consented sources', () => {
  const source = readFileSync(new URL('./capture.ts', import.meta.url), 'utf8')
  assert.match(source, /replaceMusicTrackSource\(/)
  assert.match(source, /replaceSurfaceSource\(/)
  assert.match(source, /pageConsent: input\.pageConsent/)
})

test('agent music status omits url and cover', () => {
  const status = agentMusicStatus({
    isPlaying: true,
    isEnabled: true,
    currentSong: sampleSong('Night', 'Lantern'),
    currentSongIndex: 2,
    playlistLength: 9,
    lyrics: [{ time: 0, text: 'harbour light' }],
    currentLyricIndex: 0,
  })
  assert.ok(status)
  const song = status.currentSong as Record<string, unknown>
  assert.equal(song.name, 'Night')
  assert.equal(song.album, 'Demo')
  assert.equal('url' in song, false)
  assert.equal('cover' in song, false)
  assert.equal('id' in song, false)
  assert.equal(status.currentLyric, 'harbour light')
})

test('turn capture passes a fresh selection', () => {
  const engine = readFileSync(
    new URL('../../../components/agent-panel/AgentEngine.tsx', import.meta.url),
    'utf8',
  )
  const inbound = readFileSync(new URL('./inbound.ts', import.meta.url), 'utf8')
  assert.match(engine, /turnSelectionText\(/)
  assert.match(engine, /captureTurnBody\(/)
  assert.match(engine, /agentMusicStatus\(/)
  assert.match(inbound, /turnSelectionText\(/)
  assert.match(inbound, /subscribeAgentSelection\(/)
})

test('Chat scene sources exist in capture; registry kinds remain a client ordering vocabulary', () => {
  const client = readFileSync(new URL('./registry.ts', import.meta.url), 'utf8')
  const server = readFileSync(
    new URL(
      '../../../../../backend/src/services/agent/chat_prompt.rs',
      import.meta.url,
    ),
    'utf8',
  )
  const clientKinds = quotedStringsIn(
    client,
    'export const KINDS',
    'KIND_ORDER',
  )
  assert.deepEqual([...KINDS], clientKinds)
  assert.equal(clientKinds[2], 'surface')
  assert.equal(clientKinds[1], 'pointer')
  // Chat deliberately selects pointed-at/playing/reading sources, rather
  // than forwarding every idle sensor or maintaining a second kind enum.
  const producers = ['capture.ts', 'consentedSources.ts']
    .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
    .join('\n')
  const sourceIds = new Set(
    [...producers.matchAll(/sourceId:\s*'([a-z_]+)'/g)].map(
      (match) => match[1],
    ),
  )
  const scene = server.split('match source {')[1]!.split('\n            }')[0]!
  const selected = [...scene.matchAll(/"([a-z_]+)"(?: if [^\n]+)? =>/g)].map(
    (match) => match[1],
  )
  assert.deepEqual(selected, ['music_track', 'page', 'pointer', 'surface'])
  assert.ok(selected.every((source) => sourceIds.has(source)))
  assert.match(server, /perception_view::perception_reader_text\(obj\)/)
})

test('live perception sources stay below the reader cap with slack', () => {
  const capture = readFileSync(new URL('./capture.ts', import.meta.url), 'utf8')
  const consented = readFileSync(
    new URL('./consentedSources.ts', import.meta.url),
    'utf8',
  )
  const inbound = readFileSync(new URL('./inbound.ts', import.meta.url), 'utf8')
  const view = readFileSync(
    new URL(
      '../../../../../backend/src/services/agent/perception_view.rs',
      import.meta.url,
    ),
    'utf8',
  )
  const presence = readFileSync(
    new URL(
      '../../../../../backend/src/services/agent/consciousness/presence.rs',
      import.meta.url,
    ),
    'utf8',
  )
  const chatPrompt = readFileSync(
    new URL(
      '../../../../../backend/src/services/agent/chat_prompt.rs',
      import.meta.url,
    ),
    'utf8',
  )
  const sourceIds = [
    ...new Set(
      [
        ...capture.matchAll(/sourceId:\s*'([a-z_]+)'/g),
        ...consented.matchAll(/sourceId:\s*'([a-z_]+)'/g),
      ].map((match) => match[1]),
    ),
  ]
  assert.equal(MAX_PERCEPTION_ITEMS, 12)
  assert.match(view, /MAX_PERCEPTION_ITEMS:\s*usize\s*=\s*12/)
  assert.match(
    presence,
    /take\(crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS\)/,
  )
  assert.match(
    chatPrompt,
    /take\(super::perception_view::MAX_PERCEPTION_ITEMS\)/,
  )
  assert.match(capture, /slice\(0, MAX_PERCEPTION_ITEMS\)/)
  assert.match(inbound, /slice\(0, MAX_PERCEPTION_ITEMS\)/)
  assert.ok(
    sourceIds.length <= MAX_PERCEPTION_ITEMS - 2,
    `${sourceIds.join(',')} (${sourceIds.length}) leaves no slack under cap ${MAX_PERCEPTION_ITEMS}`,
  )
})

function quotedStringsIn(
  source: string,
  startAt: string,
  stopAt: string,
): string[] {
  const start = source.indexOf(startAt)
  assert.ok(start >= 0, `missing ${startAt}`)
  const stop = source.indexOf(stopAt, start + startAt.length)
  assert.ok(stop > start, `missing ${stopAt} after ${startAt}`)
  return [...source.slice(start, stop).matchAll(/['"]([a-z_]+)['"]/g)].map(
    (match) => match[1],
  )
}
