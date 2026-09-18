import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('widget catalog load', () => {
  it('shares getSources cache and cancels apply on unmount', () => {
    const hook = readFileSync(join(dir, 'useWidgetSources.ts'), 'utf8')
    assert.match(hook, /getSources\(\)/)
    assert.match(hook, /if \(isPreview\) return/)
    assert.doesNotMatch(hook, /getSources\([^)]*signal/)
    assert.doesNotMatch(hook, /useHomeVisibilityInterval/)
    assert.match(hook, /new RequestTurn\(\)/)
    assert.match(hook, /turns\.current\.cancel\(\)/)
    assert.match(hook, /setLoading\(false\)/)
    assert.match(hook, /setFailed\(true\)/)
  })

  it('featured widget uses the shared loader instead of a private getSources loop', () => {
    const featured = readFileSync(join(dir, 'PhantasiFeaturedTile.tsx'), 'utf8')
    assert.match(featured, /useWidgetSources\(/)
    assert.doesNotMatch(featured, /from ['"][^'"]*phantasiApi['"]/)
    for (const name of ['PhantasiTopicTile.tsx', 'PhantasiSourceTile.tsx']) {
      const src = readFileSync(join(dir, name), 'utf8')
      assert.doesNotMatch(src, /useWidgetSources\(/)
      assert.doesNotMatch(src, /from ['"][^'"]*phantasiApi['"]/)
    }
  })
})
