import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const brewDir = dirname(fileURLToPath(import.meta.url))
const viewsDir = join(brewDir, '../../views')

describe('article prefetch cancel', () => {
  it('hover leave cancels the in-flight prefetch', () => {
    const feeds = readFileSync(join(brewDir, 'skin/BrewFeeds.tsx'), 'utf8')
    const board = readFileSync(join(brewDir, 'skin/BrewBoard.tsx'), 'utf8')
    const grid = readFileSync(join(brewDir, 'BrewSourceGrid.tsx'), 'utf8')
    const page = readFileSync(join(viewsDir, 'Brew.tsx'), 'utf8')
    assert.match(feeds, /onPeekEnd\?\.\(\)/)
    assert.match(board, /onPeekEnd=\{onPeekEnd\}/)
    assert.match(grid, /onPeekEnd=\{onPeekEnd\}/)
    assert.match(page, /onPeekEnd=\{cancelArticlePrefetch\}/)
  })
})
