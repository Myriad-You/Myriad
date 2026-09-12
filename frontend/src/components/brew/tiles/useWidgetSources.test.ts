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
    assert.doesNotMatch(hook, /getSources\([^)]*signal/)
    assert.match(hook, /new RequestTurn\(\)/)
    assert.match(hook, /turns\.current\.cancel\(\)/)
  })

  it('tiles use the shared loader instead of a private getSources loop', () => {
    for (const name of [
      'BrewTopicTile.tsx',
      'BrewFeaturedTile.tsx',
      'BrewSourceTile.tsx',
    ]) {
      const src = readFileSync(join(dir, name), 'utf8')
      assert.match(src, /useWidgetSources\(/)
      assert.doesNotMatch(src, /from ['"][^'"]*brewApi['"]/)
    }
  })
})
