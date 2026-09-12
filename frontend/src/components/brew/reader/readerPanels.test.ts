import assert from 'node:assert/strict'
import { it } from 'node:test'
import {
  applyExclusivePanel,
  currentReaderPanel,
  dismissReaderChrome,
  escapeWhileTyping,
  nextExclusivePanel,
  nextReaderDialogTab,
  readerDialogTrigger,
  readerPanelFlags,
  readerPopupTrigger,
} from './readerPanels'

it('toggles the same panel off and never leaves two panels open', () => {
  assert.equal(nextExclusivePanel(null, 'toc'), 'toc')
  assert.equal(nextExclusivePanel('toc', 'toc'), null)
  assert.equal(nextExclusivePanel('toc', 'podcast'), 'podcast')
  assert.deepEqual(readerPanelFlags('annotations'), {
    toc: false,
    brewlia: true,
    podcast: false,
  })
  assert.deepEqual(readerPanelFlags(null), {
    toc: false,
    brewlia: false,
    podcast: false,
  })
  assert.equal(
    currentReaderPanel({ toc: false, brewlia: true, podcast: true }),
    'annotations',
  )
  assert.deepEqual(applyExclusivePanel('toc', 'podcast'), {
    toc: false,
    brewlia: false,
    podcast: true,
  })
  assert.deepEqual(applyExclusivePanel('podcast', 'podcast'), {
    toc: false,
    brewlia: false,
    podcast: false,
  })
})

it('dialog and popup triggers share expanded/controls/haspopup', () => {
  assert.deepEqual(readerDialogTrigger(true, 'brew-reader-tool-sheet'), {
    'aria-expanded': true,
    'aria-haspopup': 'dialog',
    'aria-controls': 'brew-reader-tool-sheet',
  })
  assert.deepEqual(readerPopupTrigger(false, 'brew-reader-toc-panel'), {
    'aria-expanded': false,
    'aria-haspopup': true,
    'aria-controls': 'brew-reader-toc-panel',
  })
})

it('dismisses comments before tools, then controls, then nothing', () => {
  assert.equal(
    dismissReaderChrome({
      lightbox: true,
      popup: true,
      comments: true,
      toc: true,
      brewlia: false,
      voice: false,
      podcast: false,
      controls: true,
    }),
    'lightbox',
  )
  assert.equal(
    dismissReaderChrome({
      lightbox: false,
      popup: true,
      comments: true,
      toc: true,
      brewlia: false,
      voice: false,
      podcast: false,
      controls: true,
    }),
    'popup',
  )
  assert.equal(
    dismissReaderChrome({
      lightbox: false,
      popup: false,
      comments: true,
      toc: true,
      brewlia: false,
      voice: false,
      podcast: false,
      controls: true,
    }),
    'comments',
  )
  assert.equal(
    dismissReaderChrome({
      lightbox: false,
      popup: false,
      comments: false,
      toc: false,
      brewlia: false,
      voice: true,
      podcast: true,
      controls: true,
    }),
    'voice',
  )
  assert.equal(
    dismissReaderChrome({
      lightbox: false,
      popup: false,
      comments: false,
      toc: false,
      brewlia: false,
      voice: false,
      podcast: true,
      controls: true,
    }),
    'podcast',
  )
  assert.equal(
    dismissReaderChrome({
      lightbox: false,
      popup: false,
      comments: false,
      toc: false,
      brewlia: false,
      voice: false,
      podcast: false,
      controls: false,
    }),
    null,
  )
})

it('wraps Tab at the ends of a reader dialog', () => {
  const first = { id: 'first' } as unknown as HTMLElement
  const last = { id: 'last' } as unknown as HTMLElement
  const nodes = [first, last]
  assert.equal(nextReaderDialogTab(nodes, last, false), first)
  assert.equal(nextReaderDialogTab(nodes, first, true), last)
  assert.equal(nextReaderDialogTab(nodes, first, false), null)
  assert.equal(nextReaderDialogTab(nodes, last, true), null)
  assert.equal(nextReaderDialogTab(nodes, null, false), first)
  assert.equal(nextReaderDialogTab([], first, false), null)
})

it('lets Escape close a composer from a text field, not the reader', () => {
  assert.equal(escapeWhileTyping('popup'), 'popup')
  assert.equal(escapeWhileTyping('comments'), 'comments')
  assert.equal(escapeWhileTyping('lightbox'), 'lightbox')
  assert.equal(escapeWhileTyping('voice'), 'voice')
  assert.equal(escapeWhileTyping('toc'), null)
  assert.equal(escapeWhileTyping(null), null)
})
