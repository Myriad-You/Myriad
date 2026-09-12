import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const logicDir = join(dirname(fileURLToPath(import.meta.url)), 'logic')

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

describe('brew logic 边界', () => {
  it('logic 没有 IO：不进口 brewApi、pageData、services', () => {
    const files = listTs(logicDir)
    assert.ok(files.length > 0, 'logic 目录应有源码')
    for (const path of files) {
      const src = readFileSync(path, 'utf8')
      assert.doesNotMatch(src, /from ['"][^'"]*brewApi['"]/)
      assert.doesNotMatch(src, /from ['"][^'"]*pageData['"]/)
      assert.doesNotMatch(src, /from ['"][^'"]*\/services\//)
    }
  })
})
