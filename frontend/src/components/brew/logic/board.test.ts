/**
 * 板块划分与深链别名的单元测试。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/brew/logic/board.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  boardEntry,
  isBrewBoard,
  isSiteSource,
  resolveBoardParam,
  sourcesForBoard,
} from './board.ts'
import { makeSource } from './fixtures.ts'

describe('isSiteSource', () => {
  it('只认 source_type === link', () => {
    assert.equal(isSiteSource(makeSource({ source_type: 'link' })), true)
    assert.equal(isSiteSource(makeSource({ source_type: 'rss' })), false)
    assert.equal(isSiteSource(makeSource({ source_type: 'brewlia' })), false)
    assert.equal(isSiteSource(makeSource({ source_type: 'rsshub' })), false)
  })

  it('分类不参与判断：挂着「友情链接」的真订阅源仍属订阅板块', () => {
    const s = makeSource({ source_type: 'rss', category: '友情链接' })
    assert.equal(isSiteSource(s), false)
  })
})

describe('sourcesForBoard', () => {
  const link = makeSource({ id: 1, source_type: 'link' })
  const rss = makeSource({ id: 2, source_type: 'rss' })
  const mine = makeSource({ id: 3, source_type: 'rss', category: '我' })
  const all = [link, rss, mine]

  it('sites 只要入口型', () => {
    assert.deepEqual(
      sourcesForBoard(all, 'sites').map((s) => s.id),
      [1],
    )
  })

  it('feeds 要其余全部，包括「我」分类的自有源', () => {
    assert.deepEqual(
      sourcesForBoard(all, 'feeds').map((s) => s.id),
      [2, 3],
    )
  })

  it('notes 不是源墙，返回空', () => {
    assert.deepEqual(sourcesForBoard(all, 'notes'), [])
  })

  it('保持输入顺序 —— 排序由调用方决定', () => {
    const reversed = [mine, rss, link]
    assert.deepEqual(
      sourcesForBoard(reversed, 'feeds').map((s) => s.id),
      [3, 2],
    )
  })
})

describe('boardEntry', () => {
  it('手记直接进合并文章流', () => {
    assert.deepEqual(boardEntry('notes'), {
      view: 'category-feed',
      board: 'notes',
    })
  })

  it('订阅与站点进源墙', () => {
    assert.deepEqual(boardEntry('feeds'), { view: 'sources', board: 'feeds' })
    assert.deepEqual(boardEntry('sites'), { view: 'sources', board: 'sites' })
  })
})

describe('resolveBoardParam', () => {
  it('新参数直通', () => {
    for (const board of ['feeds', 'notes', 'sites'] as const) {
      assert.deepEqual(resolveBoardParam(board), boardEntry(board))
    }
  })

  it('旧 ?category= 的四个取值都还能落地', () => {
    assert.deepEqual(resolveBoardParam('all'), {
      view: 'sources',
      board: 'feeds',
    })
    assert.deepEqual(resolveBoardParam('friends'), {
      view: 'sources',
      board: 'sites',
    })
    assert.deepEqual(resolveBoardParam('mine'), {
      view: 'category-feed',
      board: 'notes',
    })
  })

  it('收藏不再是板块：落在订阅板块的收藏视图', () => {
    assert.deepEqual(resolveBoardParam('starred'), {
      view: 'starred',
      board: 'feeds',
    })
  })

  it('认不出的取值返回 null，不回落默认板块', () => {
    assert.equal(resolveBoardParam('nope'), null)
    assert.equal(resolveBoardParam(''), null)
  })
})

describe('isBrewBoard', () => {
  it('只认三个板块 id', () => {
    assert.equal(isBrewBoard('feeds'), true)
    assert.equal(isBrewBoard('notes'), true)
    assert.equal(isBrewBoard('sites'), true)
    assert.equal(isBrewBoard('all'), false)
    assert.equal(isBrewBoard('starred'), false)
  })
})
