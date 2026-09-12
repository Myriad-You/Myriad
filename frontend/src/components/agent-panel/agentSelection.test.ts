import assert from 'node:assert/strict'
import test from 'node:test'
import {
  MAX_SELECTION_LENGTH,
  normalizeSelection,
  SELECTION_MEMORY_MS,
  SELECTION_PREVIEW_LENGTH,
  selectionCounts,
  selectionIsFresh,
  selectionIsFromPanel,
  selectionPreview,
  turnSelectionText,
} from './agentSelection'

const NOW = 1_700_000_000_000

test('折叠排版换行，选区里的空白不当内容送上去', () => {
  assert.equal(
    normalizeSelection('  第一行\n\n  第二行\t第三行  '),
    '第一行 第二行 第三行',
  )
})

test('太长的选区截断 —— 整页正文该走页面正文那条路', () => {
  const long = 'a'.repeat(MAX_SELECTION_LENGTH + 500)
  assert.equal(normalizeSelection(long).length, MAX_SELECTION_LENGTH)
})

test('一两个字不算指着什么', () => {
  assert.equal(selectionCounts(''), false)
  assert.equal(selectionCounts('a'), false)
  assert.equal(selectionCounts('ab'), true)
})

test('预览只截到界面放得下的长度', () => {
  const text = 'x'.repeat(SELECTION_PREVIEW_LENGTH + 10)
  const preview = selectionPreview(text)
  assert.equal(preview.length, SELECTION_PREVIEW_LENGTH + 1)
  assert.ok(preview.endsWith('…'))
  assert.equal(selectionPreview('短的'), '短的')
})

test('记住的选区有时效，过久就不再当成他想指的那段', () => {
  const fresh = { text: '一段话', capturedAtMs: NOW }
  assert.equal(selectionIsFresh(fresh, NOW), true)
  assert.equal(selectionIsFresh(fresh, NOW + SELECTION_MEMORY_MS), true)
  assert.equal(selectionIsFresh(fresh, NOW + SELECTION_MEMORY_MS + 1), false)
})

test('空选区任何时候都不新鲜', () => {
  assert.equal(selectionIsFresh({ text: '', capturedAtMs: NOW }, NOW), false)
})

test('turn selection is only the still-fresh snapshot', () => {
  assert.equal(turnSelectionText(NOW), undefined)
})

test('面板里的选中不算指着页面', () => {
  const panel = {
    nodeType: 1,
    closest(sel: string) {
      return sel === '.agent-panel-overlay-anchor' ? this : null
    },
  }
  const page = {
    nodeType: 1,
    closest() {
      return null
    },
  }
  assert.equal(selectionIsFromPanel(panel as never), true)
  assert.equal(selectionIsFromPanel(page as never), false)
  assert.equal(selectionIsFromPanel(null), false)
})
