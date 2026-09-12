import type { PageContent } from '../../../contexts/PageContentContext'
import assert from 'node:assert/strict'
import { readFileSync, writeFileSync } from 'node:fs'
import test from 'node:test'
import { applyPublishedMusicState, setCurrentSongSnapshot } from '../../../contexts/currentSong'
import { capturePerceptionSnapshots } from './capture'
import { perceptionRegistry } from './registry'

interface Scene {
  id: string
  route: string
  pageConsent: boolean
  page: PageContent | null
  selection: string
  song: string | null
  playing: boolean
}

test('behavior contract exports production captures for backend consumption', (t) => {
  t.mock.timers.enable({ apis: ['Date'], now: 100_000 })
  const scenes: Scene[] = JSON.parse(readFileSync(new URL(
    '../../../../../tests/merope/scenes.json', import.meta.url,
  ), 'utf8'))
  const rows = []
  try {
    for (const scene of scenes) {
      t.mock.timers.tick(100)
      applyPublishedMusicState({
        currentSong: scene.song ? {
          id: scene.song, name: scene.song, artist: '合成测试音乐人',
          album: '测试专辑', source: 'netease', duration: 180,
          url: 'https://example.test/PRIVATE_MEDIA_URL',
          cover: 'https://example.test/PRIVATE_COVER_URL',
        } : null,
        isPlaying: scene.playing,
      })
      const perception = capturePerceptionSnapshots({ ...scene, page: scene.pageConsent ? scene.page : null })
      const payload = JSON.stringify(perception)
      assert.doesNotMatch(payload, /PRIVATE_MEDIA_URL|PRIVATE_COVER_URL/, scene.id)
      assert.equal(perception.some((item) => item.sourceId === 'page'), scene.pageConsent && scene.page !== null, scene.id)
      assert.equal(perception.some((item) => item.sourceId === 'music_track'), scene.pageConsent && scene.song !== null, scene.id)
      rows.push({ id: scene.id, data: { perception, presence: { pageVisible: true } } })
    }
    // Only the dedicated runner asks for an artifact.
    const output = process.env.MEROPE_BEHAVIOR_WIRE_PATH
    if (output) writeFileSync(output, JSON.stringify(rows))
  } finally {
    setCurrentSongSnapshot(null)
    for (const item of perceptionRegistry.active()) perceptionRegistry.forget(item.sourceId)
  }
})
