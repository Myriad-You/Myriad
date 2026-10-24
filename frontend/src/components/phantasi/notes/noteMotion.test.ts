import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))
const noteCss = readFileSync(join(dir, '../ui/css/note.css'), 'utf8')
const chrome = readFileSync(join(dir, 'NoteEditorChrome.tsx'), 'utf8')

describe('笔记编辑器元素动效', () => {
  it('正文小组件按宫格比例封顶，4 列不拉满纸面', () => {
    assert.match(noteCss, /\.note-widget \{[\s\S]*aspect-ratio:\s*var\(--note-widget-cols\)/)
    assert.match(noteCss, /\.note-widget \{[\s\S]*--note-widget-max-h:\s*20rem/)
    assert.doesNotMatch(
      noteCss,
      /\.note-widget\[data-size='4x1'\][\s\S]{0,80}width:\s*100%/,
    )
    assert.doesNotMatch(
      noteCss,
      /\.note-widget\[data-size='4x2'\],\s*\n\.note-widget\[data-size='4x4'\] \{\s*width:\s*100%/,
    )
    assert.match(
      noteCss,
      /\.note-widget\.is-selected \.note-widget__face \{\s*pointer-events:\s*auto/,
    )
    assert.match(noteCss, /\.note-widget img \{\s*margin:\s*0/)
  })

  it('选区条的弹出保住水平居中，不跟 sm-pop-in 抢 transform', () => {
    assert.match(noteCss, /@keyframes phantasi-note-bubble-in/)
    assert.match(noteCss, /translate\(-50%, -2px\)/)
    assert.match(noteCss, /phantasi-note__bubble\.is-placed[\s\S]*phantasi-note-bubble-in/)
    assert.doesNotMatch(
      noteCss,
      /\.phantasi-note__bubble\.is-placed[\s\S]{0,80}animation:\s*sm-pop-in/,
    )
  })

  it('切栏有入场；可视层只淡、不位移', () => {
    assert.match(noteCss, /\.phantasi-note__source:not\(\.is-hidden\)[\s\S]*animation:\s*sm-enter/)
    assert.match(noteCss, /\.phantasi-note__read:not\(\.is-hidden\)[\s\S]*animation:\s*sm-enter/)
    assert.match(
      noteCss,
      /\.phantasi-note__write:not\(\.phantasi-note__source,\s*\.is-hidden\)[\s\S]*animation:\s*sm-fade-in/,
    )
  })

  it('块工具条和抽屉有成套进出，弱动效档关掉', () => {
    assert.match(
      noteCss,
      /\.phantasi-note__blockbar-btn \{[\s\S]*display:\s*inline-flex[\s\S]*align-items:\s*center[\s\S]*justify-content:\s*center/,
    )
    assert.match(noteCss, /@keyframes phantasi-note-blockbar-in/)
    assert.match(noteCss, /@keyframes phantasi-note-drawer-out/)
    assert.match(noteCss, /\.phantasi-note__drawer\.is-leaving/)
    assert.match(chrome, /useDrawerPresence/)
    assert.match(chrome, /is-leaving/)
    assert.match(noteCss, /html\[data-perf-mode='exlight'\][\s\S]*phantasi-note__bubble/)
    assert.match(noteCss, /prefers-reduced-motion: reduce/)
  })

  it('插入菜单关掉先淡再卸；行槛跟手挪；弱动效也罩菜单', () => {
    assert.match(chrome, /MENU_LEAVE_MS/)
    assert.match(chrome, /usePresence\(open, MENU_LEAVE_MS\)/)
    assert.match(chrome, /coords && !leaving \? 'is-ready'/)
    assert.match(chrome, /leaving \? 'is-leaving'/)
    assert.match(chrome, /if \(!open \|\| !shown\) return/)
    assert.match(chrome, /\[open, shown, prompting, page, items\.length, reposition\]/)
    assert.match(noteCss, /@keyframes phantasi-note-menu-in/)
    assert.match(noteCss, /@keyframes phantasi-note-menu-out/)
    assert.match(noteCss, /\.phantasi-note__plus\.is-open[\s\S]*rotate\(45deg\)/)
    assert.match(
      noteCss,
      /\.phantasi-note__plus \{[\s\S]*transform var\(--sm-dur-fast[\s\S]*--sm-ease-emphasis/,
    )
    assert.doesNotMatch(
      noteCss,
      /\.phantasi-note__menu\.is-ready \.phantasi-note__menu-item[\s\S]{0,120}animation:\s*sm-fade-in/,
    )
    assert.match(noteCss, /\.phantasi-note__gutter[\s\S]*transition:[\s\S]*\btop\b/)
    assert.match(noteCss, /html\[data-perf-mode='exlight'\][\s\S]*phantasi-note__menu/)
    assert.match(noteCss, /\.phantasi-note \{[\s\S]*contain:\s*layout style/)
    assert.match(noteCss, /\.phantasi-note__title[\s\S]*field-sizing:\s*content/)
    assert.match(noteCss, /\.phantasi-note__write textarea[\s\S]*field-sizing:\s*content/)
    assert.match(noteCss, /\.phantasi-note__title[\s\S]*animation:\s*sm-fade-in/)
    assert.match(noteCss, /\.phantasi-note__read p\[data-fn\][\s\S]*animation:\s*sm-enter/)
    assert.doesNotMatch(
      noteCss,
      /\.phantasi-note__title[\s\S]{0,200}animation:\s*sm-enter/,
    )
  })
})
