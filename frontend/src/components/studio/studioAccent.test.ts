import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

it('创作台窗口不写死紫色，强调色和字色都从覆盖层来', () => {
  const css = readFileSync(join(dir, 'StudioShell.css'), 'utf8')
  assert.doesNotMatch(css, /#7c3aed|#8b5cf6|139 92 246/i)
  assert.match(css, /--cfg-accent: var\(--studio-accent,/)
  assert.match(css, /--cfg-on-accent: var\(--studio-on-accent,/)
  assert.doesNotMatch(css, /color: #fff;/)
})
