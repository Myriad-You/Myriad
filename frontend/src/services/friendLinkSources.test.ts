import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  fetchFriendLinkSources,
  shouldFetchFriendLinks,
} from './friendLinkSources.ts'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const read = (...parts: string[]) => readFileSync(join(root, ...parts), 'utf8')

describe('friend-link fetch gate', () => {
  it('only the friend-link widget is allowed to fetch', () => {
    assert.equal(shouldFetchFriendLinks('widget'), true)
    assert.equal(shouldFetchFriendLinks('overlay'), false)
    assert.equal(shouldFetchFriendLinks('layout'), false)
  })

  it('skips the friend-link catalog helper on overlay and layout surfaces', async () => {
    const overlay = await fetchFriendLinkSources('overlay')
    const layout = await fetchFriendLinkSources('layout')
    assert.deepEqual(overlay, [])
    assert.deepEqual(layout, [])
  })

  it('non-widget paths do not call the friend-link endpoint helper', () => {
    const overlay = read('components/phantasi/tiles/useWidgetSources.ts')
    const layout = read('layouts/AppLayout.tsx')
    const phantasi = read('components/phantasi/usePhantasiSources.ts')
    for (const [name, src] of [
      ['overlay', overlay],
      ['layout', layout],
      ['phantasi', phantasi],
    ] as const) {
      assert.doesNotMatch(src, /fetchFriendLinkSources/, name)
      assert.doesNotMatch(src, /category:\s*'friends'/, name)
    }
    const widget = read('components/widgets/FriendLinksWidget.tsx')
    assert.match(widget, /fetchFriendLinkSources\(\s*'widget'/)
  })
})
