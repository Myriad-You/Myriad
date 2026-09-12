import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('source discover abort', () => {
  it('add-source discover passes the form turn signal', () => {
    const form = readFileSync(
      join(dir, 'manager/modes/useAddSourceForm.ts'),
      'utf8',
    )
    const sources = readFileSync(join(dir, 'useBrewSources.ts'), 'utf8')
    const api = readFileSync(join(dir, '../../services/brewApi.ts'), 'utf8')
    assert.match(form, /onDiscover\(url, signal\)/)
    assert.match(form, /discover\(url\.trim\(\), signal\)/)
    assert.match(
      sources,
      /brewApi\.discoverSource\(url\.trim\(\), undefined, \{ signal \}\)/,
    )
    assert.match(api, /signal: options\?\.signal/)
  })

  it('opml import passes the form turn signal', () => {
    const form = readFileSync(
      join(dir, 'manager/modes/useAddSourceForm.ts'),
      'utf8',
    )
    const sources = readFileSync(join(dir, 'useBrewSources.ts'), 'utf8')
    const api = readFileSync(join(dir, '../../services/brewApi.ts'), 'utf8')
    assert.match(form, /onImportOpml\(opmlContent, signal\)/)
    assert.match(form, /importTurns\.current\.cancel\(\)/)
    assert.match(
      sources,
      /brewApi\.importOpml\(\s*content,\s*undefined,\s*signal \? \{ signal \} : undefined/,
    )
    assert.match(
      api,
      /export async function importOpml\([\s\S]*options\?: \{ signal\?: AbortSignal \}/,
    )
  })
})
