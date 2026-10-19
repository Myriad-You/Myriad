import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  closeTool,
  DEFAULT_LAYOUT,
  defaultWindow,
  fitWindow,
  formatToolSpec,
  isOverlayHotkey,
  layoutFromValue,
  layoutToValue,
  legacySectionSpec,
  nextFrameToLoad,
  openTool,
  parseToolSpec,
  rememberTool,
  toolFrameSrc,
  windowFrameSrc,
} from './studioTools.ts'

describe('tool spec', () => {
  it('工具名可带一个参数，别的都不收', () => {
    assert.deepEqual(parseToolSpec('editor-12'), { tool: 'editor', arg: '12' })
    assert.deepEqual(parseToolSpec('editor-new'), { tool: 'editor', arg: 'new' })
    assert.deepEqual(parseToolSpec('comments'), { tool: 'comments', arg: null })
    assert.deepEqual(parseToolSpec('media-kept'), { tool: 'media', arg: 'kept' })
    assert.equal(parseToolSpec('feeds'), null)
    assert.equal(parseToolSpec('/notes/12'), null)
    assert.equal(parseToolSpec(''), null)
    assert.equal(parseToolSpec(null), null)
    assert.equal(formatToolSpec({ tool: 'editor', arg: '3' }), 'editor-3')
  })

  it('窗口地址带上这一页的上下文', () => {
    assert.equal(
      toolFrameSrc({ tool: 'comments', arg: null }, { itemId: 7, docId: 3 }),
      '/embed/studio/tool/comments?item=7&doc=3',
    )
    assert.equal(
      toolFrameSrc({ tool: 'editor', arg: 'new' }, { itemId: null, docId: null }),
      '/embed/studio/tool/editor?arg=new',
    )
  })
})

describe('layout', () => {
  const viewport = { w: 1440, h: 900 }

  it('窗口不跑出视口', () => {
    const fitted = fitWindow({ x: -50, y: 0, w: 5000, h: 100, pinned: false, opacity: 3 }, viewport)
    assert.equal(fitted.x, 8)
    assert.equal(fitted.y, 56)
    assert.equal(fitted.w, 1424)
    assert.equal(fitted.h, 220)
    assert.equal(fitted.opacity, 1)
  })

  it('新窗口从工具栏右边错开排', () => {
    const first = defaultWindow('comments', 0, viewport)
    const second = defaultWindow('comments', 1, viewport)
    assert.equal(first.x, 232)
    assert.equal(second.x - first.x, 28)
    assert.equal(first.pinned, false)
  })

  it('打开已开的工具会换参数并提到最上面', () => {
    let layout = openTool(DEFAULT_LAYOUT, { tool: 'editor', arg: '1' })
    layout = openTool(layout, { tool: 'notes', arg: null })
    layout = openTool(layout, { tool: 'editor', arg: '2' })
    assert.deepEqual(
      layout.open.map(formatToolSpec),
      ['overview', 'notes', 'editor-2'],
    )
    assert.deepEqual(closeTool(layout, 'notes').open.map(formatToolSpec), ['overview', 'editor-2'])
  })

  it('窗口实际加载的地址跟着布局走：自己换内容不重载，关掉就清掉', () => {
    const none = { itemId: null, docId: null }
    const editor = (layout: ReturnType<typeof openTool>) => layout.open.find((item) => item.tool === 'editor')!
    // 新笔记拿到 id：显示的是 7，iframe 还是当初的 new，不重新加载。
    let layout = openTool(DEFAULT_LAYOUT, { tool: 'editor', arg: 'new' })
    layout = rememberTool(layout, { tool: 'editor', arg: '7' })
    assert.equal(editor(layout).arg, '7')
    assert.equal(windowFrameSrc(editor(layout), none), '/embed/studio/tool/editor?arg=new')
    // 再要求打开同一篇：只提到最上面，照旧不重载。
    layout = openTool(openTool(layout, { tool: 'notes', arg: null }), { tool: 'editor', arg: '7' })
    assert.equal(layout.open.at(-1)?.tool, 'editor')
    assert.equal(windowFrameSrc(editor(layout), none), '/embed/studio/tool/editor?arg=new')
    // 换一篇就按新的加载。
    layout = openTool(layout, { tool: 'editor', arg: '8' })
    assert.equal(windowFrameSrc(editor(layout), none), '/embed/studio/tool/editor?arg=8')
    // 关掉再开：不会载回关之前的内容。
    layout = rememberTool(layout, { tool: 'editor', arg: '9' })
    layout = openTool(closeTool(layout, 'editor'), { tool: 'editor', arg: '9' })
    assert.equal(windowFrameSrc(editor(layout), none), '/embed/studio/tool/editor?arg=9')
    // 没开着的窗口记不进来。
    assert.equal(rememberTool(DEFAULT_LAYOUT, { tool: 'editor', arg: '1' }), DEFAULT_LAYOUT)
  })

  it('存下来再读回来不变，坏数据退回默认', () => {
    const layout = {
      open: [
        { tool: 'notes' as const, arg: null, loaded: null },
        { tool: 'editor' as const, arg: '4', loaded: '4' },
      ],
      windows: { notes: { x: 300, y: 80, w: 700, h: 500, pinned: true, opacity: 0.5 } },
      toolbar: 'icons' as const,
    }
    assert.deepEqual(layoutFromValue(structuredClone(layoutToValue(layout))), layout)
    assert.deepEqual(layoutFromValue(null), DEFAULT_LAYOUT)
    assert.deepEqual(layoutFromValue({ open: ['nope', 'notes', 'notes'] }).open, [
      { tool: 'notes', arg: null, loaded: null },
    ])
  })
})

describe('hotkey and legacy addresses', () => {
  it('只认 Alt+`', () => {
    assert.equal(isOverlayHotkey({ altKey: true, ctrlKey: false, metaKey: false, code: 'Backquote' }), true)
    assert.equal(isOverlayHotkey({ altKey: false, ctrlKey: false, metaKey: false, code: 'Backquote' }), false)
    assert.equal(isOverlayHotkey({ altKey: true, ctrlKey: true, metaKey: false, code: 'Backquote' }), false)
  })

  it('旧的创作台地址对应到工具', () => {
    assert.deepEqual(legacySectionSpec(''), { tool: 'overview', arg: null })
    assert.deepEqual(legacySectionSpec('/notes/12'), { tool: 'editor', arg: '12' })
    assert.deepEqual(legacySectionSpec('/notes/new'), { tool: 'editor', arg: 'new' })
    assert.deepEqual(legacySectionSpec('/notes/'), { tool: 'notes', arg: null })
    assert.deepEqual(legacySectionSpec('/media/kept'), { tool: 'media', arg: 'kept' })
    assert.deepEqual(legacySectionSpec('/media'), { tool: 'media', arg: null })
    assert.deepEqual(legacySectionSpec('/blog'), { tool: 'settings', arg: null })
    assert.deepEqual(legacySectionSpec('/import'), { tool: 'transfer', arg: null })
    assert.deepEqual(legacySectionSpec('/whatever'), { tool: 'overview', arg: null })
  })
})

describe('nextFrameToLoad', () => {
  it('一次只加载一个，最上面的先来，看不见的不加载', () => {
    assert.equal(nextFrameToLoad(['editor', 'notes'], new Set(), null), 'editor')
    assert.equal(nextFrameToLoad(['editor', 'notes'], new Set(), 'notes'), 'notes')
    assert.equal(nextFrameToLoad(['editor', 'notes'], new Set(['editor']), null), 'notes')
    assert.equal(nextFrameToLoad(['editor'], new Set(['editor']), null), null)
    assert.equal(nextFrameToLoad(['notes'], new Set(), 'editor'), 'notes')
    assert.equal(nextFrameToLoad([], new Set(), null), null)
  })
})
