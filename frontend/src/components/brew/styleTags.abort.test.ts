import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('style-tag generate abort', () => {
  it('edit-source generate passes the form turn signal', () => {
    const edit = readFileSync(
      join(dir, 'manager/modes/EditSourceMode.tsx'),
      'utf8',
    )
    const sources = readFileSync(join(dir, 'useBrewSources.ts'), 'utf8')
    const api = readFileSync(join(dir, '../../services/brewliaApi.ts'), 'utf8')
    assert.match(edit, /onGenerateStyleTags\(source\.id, signal\)/)
    assert.match(sources, /brewApi\.generateStyleTags\(sourceId, signal\)/)
    assert.match(
      api,
      /export async function generateStyleTags\(\s*sourceId: number,\s*signal\?: AbortSignal/,
    )
  })
})
