import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('article open abort', () => {
  it('session loaders pass the turn signal into getItem', () => {
    const actions = readFileSync(join(dir, 'useBrewItemActions.ts'), 'utf8')
    const agent = readFileSync(join(dir, 'useBrewAgentOpen.ts'), 'utf8')
    const route = readFileSync(join(dir, 'useBrewItemRoute.ts'), 'utf8')
    const notes = readFileSync(join(dir, 'useBrewNotes.ts'), 'utf8')
    assert.doesNotMatch(actions, /openArticle\(\(\) => brewApi\.getItem/)
    assert.match(actions, /getItem\([^;\n]*\{ signal \}/)
    assert.match(agent, /getItem\(id, undefined, \{ signal \}\)/)
    assert.match(route, /getItem\(id, undefined, \{ signal \}\)/)
    assert.match(notes, /getItem\(id, undefined, \{ signal \}\)/)
    assert.match(notes, /saveTurn\.current\.cancel\(\)/)
  })
})
