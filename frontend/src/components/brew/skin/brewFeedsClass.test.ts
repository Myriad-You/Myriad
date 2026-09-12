import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('brew feeds 开合 class 链', () => {
  it('morphing 必须在 React className 里，重绘才能保住藏活卡', () => {
    const src = readFileSync(join(dir, 'BrewFeeds.tsx'), 'utf8')
    assert.match(src, /morphing \? ' is-sites-morphing'/)
  })

  it('文章区收拢只在开合落定后，morphing 时不 !important 压死 chrome', () => {
    const css = readFileSync(join(dir, '../ui/css/cards.css'), 'utf8')
    assert.match(
      css,
      /\.brew-feeds\.is-sites-open:not\(\.is-sites-morphing\) \.brew-feeds__items/,
    )
  })
})
