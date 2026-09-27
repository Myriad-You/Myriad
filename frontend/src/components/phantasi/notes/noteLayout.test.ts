import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { WIDGET_SIZE_KEYS } from '../../../utils/widgetSizeScale.ts'
import {
  decodeWidgetConfigAttr,
  encodeWidgetConfigAttr,
  insertColumnsMarkdown,
  insertWidgetMarkdown,
  NOTE_WIDGET_SIZES,
  noteWidgetCanConfigure,
  noteWidgetTypesInMarkdown,
  parseNoteLayout,
  parseWidgetDirective,
  serializeColumns,
} from './noteLayout.ts'

describe('parseWidgetDirective', () => {
  it('读类型和尺寸，类型收成小写', () => {
    assert.deepEqual(parseWidgetDirective(':::widget Weather 4x2'), {
      type: 'weather',
      size: '4x2',
      config: null,
    })
    assert.deepEqual(parseWidgetDirective(':::WIDGET weather 4x2'), {
      type: 'weather',
      size: '4x2',
      config: null,
    })
  })

  it('没写尺寸就 2x2；非法尺寸也回 2x2；宫格键原样留下', () => {
    assert.equal(parseWidgetDirective(':::widget quote')?.size, '2x2')
    assert.equal(parseWidgetDirective(':::widget quote 9x9')?.size, '2x2')
    assert.equal(parseWidgetDirective(':::widget quote 3x3')?.size, '3x3')
    assert.deepEqual(
      [...WIDGET_SIZE_KEYS].sort(),
      ['1x1', '1x2', '2x1', '2x2', '2x3', '2x4', '3x2', '3x3', '4x1', '4x2', '4x4'],
    )
    assert.deepEqual([...NOTE_WIDGET_SIZES].sort(), [...WIDGET_SIZE_KEYS].sort())
  })

  it('不认围栏里那种乱写', () => {
    assert.equal(parseWidgetDirective(':::widget'), null)
    assert.equal(parseWidgetDirective('widget weather 2x2'), null)
  })

  it('可选 JSON 进配置；非法 JSON 丢掉；多出来的字不当小组件', () => {
    assert.deepEqual(
      parseWidgetDirective(':::widget weather 2x2 {"city":"Tokyo"}'),
      {
        type: 'weather',
        size: '2x2',
        config: { city: 'Tokyo' },
      },
    )
    assert.deepEqual(parseWidgetDirective(':::widget weather {"city":"Tokyo"}'), {
      type: 'weather',
      size: '2x2',
      config: { city: 'Tokyo' },
    })
    assert.equal(
      parseWidgetDirective(':::widget weather 2x2 {nope}')?.config,
      null,
    )
    assert.equal(
      parseWidgetDirective(':::widget weather 2x2 {"city":')?.config,
      null,
    )
    assert.equal(
      parseWidgetDirective(':::widget weather 2x2 {"city":"Tokyo",}')?.config,
      null,
    )
    assert.equal(parseWidgetDirective(':::widget weather 2x2 hello'), null)
    assert.equal(parseWidgetDirective(':::widget weather 2x2 {}')?.config, null)
  })
})

describe('parseNoteLayout', () => {
  it('普通段落仍是文本段', () => {
    const segs = parseNoteLayout('甲\n\n乙')
    assert.equal(segs.length, 1)
    assert.equal(segs[0]?.kind, 'text')
    assert.equal(segs[0]?.kind === 'text' && segs[0].text, '甲\n\n乙')
  })

  it('切开分栏和小组件，围栏里的 ::: 不动', () => {
    const md = [
      '上',
      '',
      ':::columns',
      '左 **粗**',
      ':::col',
      '右',
      ':::',
      '',
      ':::widget weather 2x2',
      '',
      '```',
      ':::widget secret 2x2',
      '```',
    ].join('\n')
    const segs = parseNoteLayout(md)
    assert.equal(segs.map((seg) => seg.kind).join(','), 'text,columns,widget,text')
    const cols = segs[1]
    assert.equal(cols?.kind, 'columns')
    if (cols?.kind === 'columns') {
      assert.equal(cols.columns.length, 2)
      assert.match(cols.columns[0]!, /左/)
      assert.equal(cols.columns[1]!.trim(), '右')
    }
    const widget = segs[2]
    assert.equal(widget?.kind, 'widget')
    if (widget?.kind === 'widget') {
      assert.equal(widget.type, 'weather')
      assert.equal(widget.size, '2x2')
      assert.equal(widget.config, null)
    }
    const tail = segs[3]
    assert.equal(tail?.kind, 'text')
    if (tail?.kind === 'text') {
      assert.match(tail.text, /:::widget secret/)
    }
    assert.deepEqual(noteWidgetTypesInMarkdown(md), ['weather'])
  })

  it('分栏未闭合也收齐已读到的栏', () => {
    const segs = parseNoteLayout(':::columns\n甲\n:::col\n乙')
    assert.equal(segs[0]?.kind, 'columns')
    if (segs[0]?.kind === 'columns') {
      assert.equal(segs[0].columns.length, 2)
    }
  })
})

describe('serialize', () => {
  it('空两栏能插进去再读回来', () => {
    const md = insertColumnsMarkdown()
    const segs = parseNoteLayout(md)
    assert.equal(segs[0]?.kind, 'columns')
    if (segs[0]?.kind === 'columns') {
      assert.equal(segs[0].columns.length, 2)
    }
  })

  it('栏里的正文往返还在', () => {
    const md = serializeColumns(['左 **粗**', '右'])
    const back = parseNoteLayout(md)
    assert.equal(back[0]?.kind, 'columns')
    if (back[0]?.kind === 'columns') {
      assert.match(back[0].columns[0]!, /左 \*\*粗\*\*/)
      assert.equal(back[0].columns[1], '右')
    }
  })

  it('小组件一行就能插', () => {
    assert.equal(insertWidgetMarkdown('Weather', '4x1'), ':::widget weather 4x1')
    assert.equal(
      insertWidgetMarkdown('github-repos', '2x2', { repo: 'owner/name' }),
      ':::widget github-repos 2x2 {"repo":"owner/name"}',
    )
  })

  it('配置属性按 URI 编码，读得回来', () => {
    const encoded = encodeWidgetConfigAttr({ city: 'Tokyo' })
    assert.equal(encoded, '%7B%22city%22%3A%22Tokyo%22%7D')
    assert.deepEqual(decodeWidgetConfigAttr(encoded), { city: 'Tokyo' })
    assert.deepEqual(decodeWidgetConfigAttr('{"city":"Tokyo"}'), { city: 'Tokyo' })
    assert.equal(decodeWidgetConfigAttr('{nope}'), null)
    assert.deepEqual(
      decodeWidgetConfigAttr('{&quot;text&quot;:&quot;&amp;lt;b&amp;gt;&quot;}'),
      { text: '&lt;b&gt;' },
    )
    assert.equal(encodeWidgetConfigAttr({}), null)
  })

  it('配置入口认声明式 settings 和长按那几类', () => {
    assert.equal(
      noteWidgetCanConfigure({
        id: 'github-repos',
        name: 'GitHub',
        defaultSize: '2x2',
        component: () => null,
        settings: [{ key: 'repo', type: 'input', label: 'repo' }],
      }),
      true,
    )
    assert.equal(
      noteWidgetCanConfigure({
        id: 'game-presence',
        name: 'Game',
        defaultSize: '4x2',
        component: () => null,
      }),
      true,
    )
    assert.equal(
      noteWidgetCanConfigure({
        id: 'weather',
        name: 'Weather',
        defaultSize: '2x2',
        component: () => null,
      }),
      false,
    )
  })
})
