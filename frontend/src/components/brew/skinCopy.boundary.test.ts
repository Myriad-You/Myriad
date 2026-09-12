import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const skinDir = join(dirname(fileURLToPath(import.meta.url)), 'skin')

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

describe('brew skin copy', () => {
  it('does not Record-cast t.brew', () => {
    for (const path of listTs(skinDir)) {
      const src = readFileSync(path, 'utf8')
      assert.doesNotMatch(src, /t\.brew as unknown as Record/)
    }
  })
})
