import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('brew manager 栏边界', () => {
  it('栏波次只进口 ui，不进口 skin', () => {
    const src = readFileSync(join(dir, 'bar.tsx'), 'utf8')
    assert.match(src, /from ['"]\.\.\/ui\//)
    assert.doesNotMatch(src, /from ['"]\.\.\/skin\//)
  })

  it('控制栏不进口 skin，也不直接碰 ZIP', () => {
    const src = readFileSync(join(dir, 'BrewControls.tsx'), 'utf8')
    assert.doesNotMatch(src, /from ['"]\.\.\/skin\//)
    assert.doesNotMatch(src, /jszip/i)
    assert.doesNotMatch(src, /from ['"]\.\/brewpackIo['"]/)
    assert.match(src, /from ['"]\.\/useBrewpack['"]/)
    assert.match(src, /from ['"]\.\/useBarWave['"]/)
    assert.doesNotMatch(src, /brewApi\.updateSource/)
    assert.doesNotMatch(src, /brewApi\.importOpml/)
    assert.doesNotMatch(src, /brewApi\.deleteSource/)
    assert.doesNotMatch(src, /brewApi\.discoverSource/)
    assert.doesNotMatch(src, /from ['"].*brewApi['"]/)
  })
})
