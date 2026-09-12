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

describe('brew/skin 边界', () => {
  it('皮不进口 brewApi / pageData / manager', () => {
    const files = walk(dir, /\.(ts|tsx)$/).filter(
      (file) => !file.endsWith('.test.ts'),
    )
    assert.ok(files.length > 0)
    for (const file of files) {
      const src = readFileSync(file, 'utf8')
      assert.doesNotMatch(src, /from ['"].*brewApi['"]/)
      assert.doesNotMatch(src, /from ['"].*pageData['"]/)
      assert.doesNotMatch(src, /from ['"].*useBoardPage['"]/)
      assert.doesNotMatch(src, /from ['"].*useBrewSources['"]/)
      assert.doesNotMatch(src, /from ['"].*useBrewItems['"]/)
      assert.doesNotMatch(src, /from ['"].*useBrewStarred['"]/)
      assert.doesNotMatch(src, /from ['"].*useBrewNotes['"]/)
      assert.doesNotMatch(src, /from ['"].*useBrewSeo['"]/)
      assert.doesNotMatch(src, /from ['"]\.\.\/manager/)
    }
  })
})
