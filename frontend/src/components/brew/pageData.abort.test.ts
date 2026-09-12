import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('pageData abort', () => {
  it('feed stories and home notes pass AbortSignal through getItemPreviews', () => {
    const src = readFileSync(join(dir, 'pageData.ts'), 'utf8')
    assert.match(src, /export async function loadFeedStories\([\s\S]*signal\?: AbortSignal/)
    assert.match(src, /export async function loadHomeBoardNotes\([\s\S]*signal\?: AbortSignal/)
    assert.match(src, /signal \? \{ signal \} : undefined/)
    assert.match(src, /if \(signal\) \{\s*const notes = await load\(\)/)
    assert.match(src, /if \(signal\) return load\(\)/)
    assert.match(src, /if \(!signal\?\.aborted\) putFeedStories/)
  })

  it('board page aborts in-flight feed and notes loads on retarget', () => {
    const src = readFileSync(join(dir, 'useBoardPage.ts'), 'utf8')
    assert.match(src, /new AbortController\(\)/)
    assert.match(src, /loadFeedStories\(readySourceId, stamp, controller\.signal\)/)
    assert.match(src, /loadHomeBoardNotes\(sourcesRef\.current, controller\.signal\)/)
    assert.match(src, /controller\.abort\(\)/)
  })
})
