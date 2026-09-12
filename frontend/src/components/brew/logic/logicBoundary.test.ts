import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

function walk(root: string, suffix: RegExp): string[] {
  const out: string[] = []
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const next = join(root, entry.name)
    if (entry.isDirectory()) out.push(...walk(next, suffix))
    else if (suffix.test(entry.name)) out.push(next)
  }
  return out
}

describe('brew/logic 边界', () => {
  it('纯函数不进口 brewApi / pageData / manager / skin', () => {
    const files = walk(dir, /\.(ts|tsx)$/).filter(
      (file) => !file.endsWith('.test.ts'),
    )
    assert.ok(files.length > 0)
    for (const file of files) {
      const src = readFileSync(file, 'utf8')
      assert.doesNotMatch(src, /from ['"].*brewApi['"]/)
      assert.doesNotMatch(src, /from ['"].*brewliaApi['"]/)
      assert.doesNotMatch(src, /from ['"].*pageData['"]/)
      assert.doesNotMatch(src, /\bfetch\s*\(/)
      assert.doesNotMatch(src, /XMLHttpRequest/)
      assert.doesNotMatch(src, /from ['"]\.\.\/manager/)
      assert.doesNotMatch(src, /from ['"]\.\.\/skin/)
    }
  })
})
