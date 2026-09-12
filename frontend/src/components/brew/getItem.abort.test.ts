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

describe('brew getItem abort', () => {
  it('callers pass a signal except the brewApi definition', () => {
    const hits: string[] = []
    for (const path of listTs(brewDir)) {
      const src = readFileSync(path, 'utf8')
      const stripped = src
        .replaceAll(/\/\*[\s\S]*?\*\//g, '')
        .replaceAll(/\/\/.*$/gm, '')
        .replaceAll(/globalThis\.localStorage\?\.getItem\(/g, 'storageGet(')
        .replaceAll(/localStorage\.getItem\(/g, 'storageGet(')
        .replaceAll(/sessionStorage\.getItem\(/g, 'storageGet(')
        .replaceAll(/\bstorage\.getItem\(/g, 'storageGet(')
      for (const match of stripped.matchAll(/(?:brewApi\.)?getItem\([^,)\n]+\)/g)) {
        hits.push(`${path.slice(brewDir.length + 1)}: ${match[0]}`)
      }
    }
    assert.deepEqual(hits, [])
  })
})
