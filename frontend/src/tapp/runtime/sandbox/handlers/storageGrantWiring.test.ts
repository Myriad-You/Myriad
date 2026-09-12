import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

describe('registerStorageHandlers grant wiring', () => {
  it('sends Runtime Grant only for subject storage, not shared/private', () => {
    const source = readFileSync(
      fileURLToPath(new URL('./baseHandlers.ts', import.meta.url)),
      'utf8',
    )
    const blocks = Iterator.from(
      source.matchAll(
        /registerFullKvHandlers\(\s*bridge,\s*tappId,\s*'(storage|shared|private)',[\s\S]*?withGrant:\s*(true|false)/g,
      ),
    )
      .map((match) => [match[1], match[2]])
      .toArray()
    assert.deepEqual(
      blocks,
      [
        ['storage', 'true'],
        ['shared', 'false'],
        ['private', 'false'],
      ],
    )
  })
})
