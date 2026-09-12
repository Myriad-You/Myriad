import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

function src(name: string): string {
  return readFileSync(join(dir, name), 'utf8')
}

describe('reader overlay ids', () => {
  it('panels and triggers share constants, not string literals', () => {
    const mobile = src('MobileReaderBar.tsx')
    const left = src('ReaderLeftPanel.tsx')
    const right = src('ReaderRightPanel.tsx')
    const comments = src('CommentsListPanel.tsx')
    assert.match(mobile, /READER_TOOL_SHEET_ID/)
    assert.match(mobile, /READER_COMMENTS_PANEL_ID/)
    assert.doesNotMatch(mobile, /id="brew-reader-tool-sheet"/)
    assert.match(left, /READER_TOC_PANEL_ID/)
    assert.match(left, /READER_ANNOTATIONS_PANEL_ID/)
    assert.match(left, /READER_PODCAST_PANEL_ID/)
    assert.doesNotMatch(left, /id="brew-reader-toc-panel"/)
    assert.match(right, /READER_COMMENTS_PANEL_ID/)
    assert.match(comments, /READER_COMMENTS_PANEL_ID/)
    assert.doesNotMatch(comments, /id="brew-comments-panel"/)
    assert.match(mobile, /useReaderDialogFocus\(/)
    assert.match(comments, /useReaderDialogFocus\(/)
  })

  it('dialog triggers declare haspopup', () => {
    const mobile = src('MobileReaderBar.tsx')
    const right = src('ReaderRightPanel.tsx')
    const left = src('ReaderLeftPanel.tsx')
    assert.equal((mobile.match(/readerDialogTrigger\(/g) ?? []).length, 4)
    assert.equal((mobile.match(/readerPopupTrigger\(/g) ?? []).length, 1)
    assert.equal((right.match(/readerDialogTrigger\(/g) ?? []).length, 1)
    assert.equal((left.match(/readerPopupTrigger\(/g) ?? []).length, 3)
  })
})
