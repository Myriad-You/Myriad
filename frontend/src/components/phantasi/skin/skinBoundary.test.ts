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

describe('phantasi/skin 边界', () => {
  it('皮不进口 phantasiApi / pageData / manager', () => {
    const files = walk(dir, /\.(ts|tsx)$/).filter(
      (file) => !file.endsWith('.test.ts'),
    )
    assert.ok(files.length > 0)
    for (const file of files) {
      const src = readFileSync(file, 'utf8')
      assert.doesNotMatch(src, /from ['"].*phantasiApi['"]/)
      assert.doesNotMatch(src, /from ['"].*pageData['"]/)
      assert.doesNotMatch(src, /from ['"].*useBoardPage['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiSources['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiItems['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiStarred['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiNotes['"]/)
      assert.doesNotMatch(src, /from ['"].*usePhantasiSeo['"]/)
      assert.doesNotMatch(src, /from ['"]\.\.\/manager/)
    }
  })

  it('创作侧皮还在，订阅导入不在这一层', () => {
    for (const file of [
      'PhantasiWorkbenchNotes.tsx',
      'PhantasiWorkbenchComments.tsx',
      'PhantasiWorkbenchReviews.tsx',
      'PhantasiWorkbenchMedia.tsx',
      'PhantasiWorkbenchIo.tsx',
      'PhantasiWorkbenchChrome.tsx',
    ]) {
      assert.ok(readFileSync(join(dir, file), 'utf8').length > 0, file)
    }
    const io = readFileSync(join(dir, 'PhantasiWorkbenchIo.tsx'), 'utf8')
    assert.match(io, /if \(pane !== 'notesIo'\) return null/)
    assert.match(io, /workbenchWordpress/)
    assert.doesNotMatch(io, /feedsIo|pipack|opml/)
    assert.doesNotMatch(io, /from ['"]\.\.\/manager/)
  })
})
