import assert from 'node:assert/strict'
import { beforeEach, describe, it } from 'node:test'
import {
  clearNoteDraft,
  draftDiffersFrom,
  NOTE_DRAFT_TTL_MS,
  noteDraftKey,
  prefixLines,
  readNoteDraft,
  wrapSelection,
  writeNoteDraft,
} from './noteDraft.ts'

/** node:test 没有 localStorage。 */
function installStorage(): Map<string, string> {
  const store = new Map<string, string>()
  ;(globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  }
  return store
}

describe('草稿存取', () => {
  let store: Map<string, string>
  beforeEach(() => {
    store = installStorage()
  })

  it('写进去能读回来', () => {
    writeNoteDraft('new', { title: '标题', contentMd: '正文' }, 1000)
    assert.deepEqual(readNoteDraft('new', 1000), {
      title: '标题',
      contentMd: '正文',
      savedAt: 1000,
    })
  })

  it('新草稿和已发布的草稿互不覆盖', () => {
    writeNoteDraft('new', { title: '甲', contentMd: 'a' }, 1)
    writeNoteDraft(7, { title: '乙', contentMd: 'b' }, 1)
    assert.equal(readNoteDraft('new', 1)?.title, '甲')
    assert.equal(readNoteDraft(7, 1)?.title, '乙')
  })

  it('过期草稿不再恢复', () => {
    writeNoteDraft('new', { title: '标题', contentMd: '正文' }, 0)
    assert.notEqual(readNoteDraft('new', NOTE_DRAFT_TTL_MS), null)
    assert.equal(readNoteDraft('new', NOTE_DRAFT_TTL_MS + 1), null)
  })

  it('坏数据当没有草稿，不抛', () => {
    store.set(noteDraftKey('new'), '{ 不是 json')
    assert.equal(readNoteDraft('new'), null)
    store.set(noteDraftKey('new'), '{"title":"x"}')
    assert.equal(readNoteDraft('new'), null)
  })

  it('清掉之后读不到', () => {
    writeNoteDraft(3, { title: '标题', contentMd: '正文' }, 1)
    clearNoteDraft(3)
    assert.equal(readNoteDraft(3, 1), null)
  })

  it('localStorage 不可用时读写都不抛', () => {
    ;(globalThis as { localStorage?: unknown }).localStorage = {
      getItem: () => {
        throw new Error('blocked')
      },
      setItem: () => {
        throw new Error('blocked')
      },
      removeItem: () => {
        throw new Error('blocked')
      },
    }
    assert.equal(readNoteDraft('new'), null)
    writeNoteDraft('new', { title: 'a', contentMd: 'b' })
    clearNoteDraft('new')
  })
})

describe('draftDiffersFrom', () => {
  it('内容相同就不算有改动', () => {
    const draft = { title: '标题', contentMd: '正文', savedAt: 1 }
    assert.equal(
      draftDiffersFrom(draft, { title: '标题', contentMd: '正文' }),
      false,
    )
  })

  it('只差首尾空白也不算改动', () => {
    const draft = { title: ' 标题 ', contentMd: '正文\n', savedAt: 1 }
    assert.equal(
      draftDiffersFrom(draft, { title: '标题', contentMd: '正文' }),
      false,
    )
  })

  it('正文变了就算改动', () => {
    const draft = { title: '标题', contentMd: '新正文', savedAt: 1 }
    assert.equal(
      draftDiffersFrom(draft, { title: '标题', contentMd: '正文' }),
      true,
    )
  })

  it('没有草稿就没有改动', () => {
    assert.equal(draftDiffersFrom(null, { title: 'a', contentMd: 'b' }), false)
  })
})

describe('wrapSelection', () => {
  it('包住选中的字，并保持选中', () => {
    const r = wrapSelection('这是正文', 2, 4, '**', '**')
    assert.equal(r.value, '这是**正文**')
    assert.equal(r.value.slice(r.selectionStart, r.selectionEnd), '正文')
  })

  it('没选区时插入占位符并选中它', () => {
    const r = wrapSelection('', 0, 0, '**', '**', '粗体')
    assert.equal(r.value, '**粗体**')
    assert.equal(r.value.slice(r.selectionStart, r.selectionEnd), '粗体')
  })

  it('没选区也没占位符时只插入标记', () => {
    const r = wrapSelection('ab', 1, 1, '`', '`')
    assert.equal(r.value, 'a``b')
    assert.equal(r.selectionStart, r.selectionEnd)
  })
})

describe('prefixLines', () => {
  it('给光标所在行加前缀', () => {
    const r = prefixLines('标题行', 1, 1, '## ')
    assert.equal(r.value, '## 标题行')
  })

  it('多行选区每一行都加', () => {
    const r = prefixLines('甲\n乙\n丙', 0, 5, '- ')
    assert.equal(r.value, '- 甲\n- 乙\n- 丙')
  })

  it('选区停在行边界上不带上下一行', () => {
    // 选到行边界为止，下一行不加前缀。
    assert.equal(prefixLines('甲\n乙\n丙', 0, 3, '- ').value, '- 甲\n- 乙\n丙')
    // 选区含行尾换行时，下一行仍不加前缀。
    assert.equal(prefixLines('甲\n乙\n丙', 0, 4, '- ').value, '- 甲\n- 乙\n丙')
  })

  it('已经全部有前缀时再点一次是去掉 —— 按钮可切换', () => {
    const r = prefixLines('- 甲\n- 乙', 0, 5, '- ')
    assert.equal(r.value, '甲\n乙')
  })

  it('只有部分行有前缀时补齐，不是去掉', () => {
    const r = prefixLines('- 甲\n乙', 0, 5, '- ')
    assert.equal(r.value, '- - 甲\n- 乙')
  })

  it('不动选区之外的行', () => {
    const r = prefixLines('头\n甲\n尾', 2, 3, '> ')
    assert.equal(r.value, '头\n> 甲\n尾')
  })
})
