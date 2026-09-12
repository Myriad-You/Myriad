import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  boardEntry,
  collectSourceCategories,
  eatSearchKeys,
  filterLaneItems,
  filterSourcesByQuery,
  isBrewBoard,
  isFriendSource,
  isNotesSource,
  isSiteSource,
  refreshableSourceCount,
  resolveBoardParam,
  showsFilterLane,
  sortSourcesForBoard,
  sourcesForBoard,
  viewForBoardEntry,
} from './board.ts'
import { makeSource } from './fixtures.ts'

describe('isSiteSource', () => {
  it('只认 source_type === link', () => {
    assert.equal(isSiteSource(makeSource({ source_type: 'link' })), true)
    assert.equal(isSiteSource(makeSource({ source_type: 'rss' })), false)
    assert.equal(
      refreshableSourceCount([
        makeSource({ source_type: 'link' }),
        makeSource({ source_type: 'rss' }),
      ]),
      1,
    )
  })
})

describe('isFriendSource', () => {
  it('入口型算朋友', () => {
    assert.equal(isFriendSource(makeSource({ source_type: 'link' })), true)
  })

  it('挂着「友情链接」的 RSS 算朋友', () => {
    const s = makeSource({ source_type: 'rss', category: '友情链接' })
    assert.equal(isFriendSource(s), true)
    assert.equal(isSiteSource(s), false)
  })

  it('认 friend_links / Friend Links 别名', () => {
    assert.equal(
      isFriendSource(makeSource({ source_type: 'rss', category: 'friend_links' })),
      true,
    )
    assert.equal(
      isFriendSource(makeSource({ source_type: 'rss', category: 'Friend Links' })),
      true,
    )
  })

  it('自有源不去朋友们', () => {
    const s = makeSource({
      source_type: 'rss',
      category: '我,友情链接',
    })
    assert.equal(isNotesSource(s), true)
    assert.equal(isFriendSource(s), false)
  })
})

describe('isNotesSource', () => {
  it('手记源和「我」分类都算', () => {
    assert.equal(
      isNotesSource(makeSource({ source_type: 'note', category: '我' })),
      true,
    )
    assert.equal(
      isNotesSource(makeSource({ source_type: 'rss', category: '我' })),
      true,
    )
    assert.equal(
      isNotesSource(makeSource({ source_type: 'rss', category: '科技' })),
      false,
    )
    assert.equal(
      isNotesSource(makeSource({ source_type: 'rss', category: 'mine' })),
      true,
    )
  })
})

describe('sourcesForBoard', () => {
  const link = makeSource({ id: 1, source_type: 'link' })
  const rss = makeSource({ id: 2, source_type: 'rss' })
  const mine = makeSource({ id: 3, source_type: 'rss', category: '我' })
  const friendRss = makeSource({
    id: 5,
    source_type: 'rss',
    category: '友情链接',
  })
  const note = makeSource({ id: 4, source_type: 'note', category: '我' })
  const all = [link, rss, mine, friendRss, note]

  it('sites 收入口和友情链接订阅，不收自有源', () => {
    assert.deepEqual(
      sourcesForBoard(all, 'sites').map((s) => s.id),
      [1, 5],
    )
  })

  it('feeds 不再收朋友源', () => {
    assert.deepEqual(
      sourcesForBoard(all, 'feeds').map((s) => s.id),
      [2, 3, 4],
    )
  })

  it('notes 收手记源和「我」分类', () => {
    assert.deepEqual(
      sourcesForBoard(all, 'notes').map((s) => s.id),
      [3, 4],
    )
  })
})

describe('boardEntry', () => {
  it('三个板块都进源墙', () => {
    assert.deepEqual(boardEntry('notes'), { view: 'sources', board: 'notes' })
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
      view: 'sources',
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
  })
})

describe('viewForBoardEntry / eatSearchKeys / filterLaneItems', () => {
  it('游客收藏深链降到源墙；query 落地后吃掉', () => {
    assert.equal(
      viewForBoardEntry({ view: 'starred', board: 'feeds' }, false),
      'sources',
    )
    assert.equal(
      viewForBoardEntry({ view: 'starred', board: 'feeds' }, true),
      'starred',
    )
    const next = eatSearchKeys(
      new URLSearchParams('board=notes&keep=1'),
      ['board', 'category'],
    )
    assert.equal(next.get('board'), null)
    assert.equal(next.get('keep'), '1')
    assert.deepEqual(filterLaneItems('sources', [1], []), [])
    assert.deepEqual(filterLaneItems('starred', [1], []), [1])
    assert.equal(showsFilterLane('starred', false, true), true)
    assert.equal(showsFilterLane('starred', false, false), false)
    assert.equal(showsFilterLane('topic-feed', true, false), true)
  })
})

describe('isBrewBoard', () => {
  it('只认三个板块 id', () => {
    assert.equal(isBrewBoard('feeds'), true)
    assert.equal(isBrewBoard('friends'), false)
    assert.equal(isBrewBoard('starred'), false)
  })
})

describe('collectSourceCategories', () => {
  it('拆逗号、去空、去重', () => {
    assert.deepEqual(
      collectSourceCategories([
        makeSource({ category: '技术, 我' }),
        makeSource({ category: '技术' }),
        makeSource({ category: null }),
      ]).toSorted(),
      ['我', '技术'],
    )
  })
})

describe('filterSourcesByQuery', () => {
  it('空词原样拷贝', () => {
    const sources = [makeSource({ name: 'A' })]
    const next = filterSourcesByQuery(sources, '  ')
    assert.deepEqual(next.map((s) => s.id), sources.map((s) => s.id))
    assert.notEqual(next, sources)
  })

  it('按名 / 址 / 简介收', () => {
    const sources = [
      makeSource({ name: '星辰博客', url: 'https://a.com', description: null }),
      makeSource({ name: '其他', url: 'https://b.com/feed', description: 'hello' }),
    ]
    assert.equal(filterSourcesByQuery(sources, '星辰')[0]?.name, '星辰博客')
    assert.equal(filterSourcesByQuery(sources, 'B.COM')[0]?.name, '其他')
    assert.equal(filterSourcesByQuery(sources, 'hello')[0]?.name, '其他')
  })
})

describe('sortSourcesForBoard', () => {
  it('拼音按名字', () => {
    const sorted = sortSourcesForBoard(
      [
        makeSource({ id: 2, name: '星辰' }),
        makeSource({ id: 1, name: '白的' }),
      ],
      'pinyin',
      'guest',
      0,
      'zh-CN',
    )
    assert.deepEqual(
      sorted.map((s) => s.name),
      ['白的', '星辰'],
    )
  })

  it('分类按主分类再按名', () => {
    const sorted = sortSourcesForBoard(
      [
        makeSource({ id: 2, name: 'one-b', category: 'aaa' }),
        makeSource({ id: 1, name: 'two', category: 'zzz' }),
        makeSource({ id: 3, name: 'one-a', category: 'aaa' }),
      ],
      'category',
      'guest',
      0,
    )
    assert.deepEqual(
      sorted.map((s) => s.name),
      ['one-a', 'one-b', 'two'],
    )
  })
})
