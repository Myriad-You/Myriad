import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))
const preview = readFileSync(join(dir, 'useNoteEditorPreview.ts'), 'utf8')
const view = readFileSync(join(dir, 'NoteEditorView.tsx'), 'utf8')

describe('useNoteEditorPreview', () => {
  it('正文区加载完才挂上、也可能整块重挂，灌正文的两处都跟着 loading 和节点重跑', () => {
    // 前提：可视正文和预览都在加载完才挂的 <article> 里。
    assert.match(view, /\{loading \? \([\s\S]*?<article className="phantasi-note__paper">[\s\S]*?ref=\{bindVisual\}[\s\S]*?ref=\{bindPreview\}[\s\S]*?<\/article>/)
    // 原文在加载期间就到了的话，挂上之后要再灌一次，不然正文一直是空的。
    assert.match(preview, /if \(loading \|\| pane !== 'visual'\) return/)
    assert.match(preview, /if \(loading \|\| pane !== 'preview'\) return\n\s*const el = previewRef\.current/)
    assert.match(preview, /\}, \[contentMd, contentMdRef, loading, pane, visualEditing, visualNode, visualRef,/)
    assert.match(preview, /\}, \[contentMd, html, loading, pane, previewMdRef, previewNode, previewRef,/)
  })
})
