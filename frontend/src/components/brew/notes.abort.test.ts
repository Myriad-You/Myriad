import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('note editor abort', () => {
  it('draft load and preview pass AbortSignal', () => {
    const editor = readFileSync(join(dir, 'notes/NoteEditor.tsx'), 'utf8')
    const api = readFileSync(join(dir, '../../services/brewApi.ts'), 'utf8')
    assert.match(api, /export async function getNoteDraft\(\s*id: number,\s*signal\?: AbortSignal/)
    assert.match(editor, /getNoteDraft\(noteId, controller\.signal\)/)
    assert.match(editor, /previewNote\(contentMd, controller\.signal\)/)
    assert.match(editor, /controller\.abort\(\)/)
  })
})
