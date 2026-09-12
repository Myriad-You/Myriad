import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const brewDir = dirname(fileURLToPath(import.meta.url))
const skinDir = join(brewDir, 'skin')

function listTs(dir: string): string[] {
  const names: string[] = []
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) {
      names.push(...listTs(path))
      continue
    }
    if (!/\.(ts|tsx)$/.test(name) || name.endsWith('.test.ts') || name.endsWith('.test.tsx')) {
      continue
    }
    names.push(path)
  }
  return names
}

describe('brew skin 边界', () => {
  it('skin 不进口 brewApi、pageData、manager', () => {
    const files = listTs(skinDir)
    assert.ok(files.length > 0, 'skin 目录应有源码')
    for (const path of files) {
      const src = readFileSync(path, 'utf8')
      assert.doesNotMatch(src, /from ['"][^'"]*brewApi['"]/)
      assert.doesNotMatch(src, /from ['"][^'"]*pageData['"]/)
      assert.doesNotMatch(src, /from ['"]\.\.\/manager\//)
    }
  })
})
