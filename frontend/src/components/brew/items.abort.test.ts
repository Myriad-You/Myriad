import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('starred/topic list abort', () => {
  it('aborts in-flight previews when the list retargets', () => {
    const src = readFileSync(join(dir, 'useBrewItems.ts'), 'utf8')
    assert.match(src, /new RequestTurn\(\)/)
    assert.match(
      src,
      /getItemPreviews\(\s*\{[\s\S]*\},\s*undefined,\s*\{\s*signal\s*\}/,
    )
    assert.match(src, /turns\.current\.cancel\(\)/)
    assert.match(src, /if \(signal\.aborted \|\| requestId !== loadRequestIdRef\.current\) return/)
  })
})
