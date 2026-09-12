import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const brewDir = dirname(fileURLToPath(import.meta.url))

function listTs(dir: string): string[] {
  const names: string[] = []
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) {
      names.push(...listTs(path))
      continue
    }
    if (!/\.(ts|tsx)$/.test(name) || name.includes('.test.')) continue
    names.push(path)
  }
  return names
}

describe('brew copy casts', () => {
  it('does not Record-cast t.brew', () => {
    const hits: string[] = []
    for (const path of listTs(brewDir)) {
      const src = readFileSync(path, 'utf8')
      if (/t\.brew as(?: unknown as)? Record/.test(src)) {
        hits.push(path.slice(brewDir.length + 1))
      }
    }
    assert.deepEqual(hits, [])
  })
})
