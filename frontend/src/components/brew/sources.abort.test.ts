import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('source catalog abort contract', () => {
  it('getSources and getStats still accept an AbortSignal', () => {
    const api = readFileSync(join(dir, '../../services/brewApi.ts'), 'utf8')
    assert.match(
      api,
      /export async function getSources\([\s\S]*options\?: \{ signal\?: AbortSignal \}/,
    )
    assert.match(
      api,
      /export async function getStats\([\s\S]*options\?: \{ signal\?: AbortSignal \}/,
    )
  })

  it('catalog hooks cancel the in-flight turn on unmount', () => {
    const src = readFileSync(join(dir, 'useBrewSources.ts'), 'utf8')
    assert.match(src, /sourceTurns\.current\.cancel\(/)
    assert.match(src, /statsTurns\.current\.cancel\(/)
  })
})
