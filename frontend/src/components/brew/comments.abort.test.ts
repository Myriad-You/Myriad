import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('comments abort', () => {
  it('list and replies pass article abort into brewApi', () => {
    const hook = readFileSync(
      join(dir, 'reader/hooks/useComments.ts'),
      'utf8',
    )
    const api = readFileSync(join(dir, '../../services/brewApi.ts'), 'utf8')
    assert.match(hook, /getComments\(itemId, undefined, \{ signal \}\)/)
    assert.match(hook, /getCommentReplies\(commentId, undefined, \{[\s\S]*signal/)
    assert.match(hook, /itemAbort\.current = controller/)
    assert.match(api, /signal: options\?\.signal/)
  })
})
