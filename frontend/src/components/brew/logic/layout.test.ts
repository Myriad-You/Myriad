/**
 * 构图与尺寸派生的单元测试。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/brew/logic/layout.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { daysAgo, makePreviews, makeSource, NOW } from './fixtures.ts'
import {
  allowedTileSizes,
  BREW_TILE_SIZES,
  cardSizeForTile,
  CONTENT_TILE_SIZES,
  downgradeForBand,
  lockedTileSize,
  nextLockedSize,
  SITE_TILE_SIZES,
  tileLayout,
  tileSize,
  topicTileSize,
} from './layout.ts'

describe('tileLayout', () => {
  it('友链一律 icon —— 即使未读爆表、即使沉寂', () => {
    const s = makeSource({
      source_type: 'link',
      unread_count: 999,
      last_success_at: daysAgo(900),
      pulses: [1, 2, 3, 4, 5, 6, 7],
    })
    assert.equal(tileLayout(s, 'admin', NOW), 'icon')
    assert.equal(tileLayout(s, 'guest', NOW), 'icon')
  })

  it('登录角色 + 未读 ≥ 20 → numeric', () => {
    const s = makeSource({ unread_count: 20, recent_items: makePreviews(8) })
    assert.equal(tileLayout(s, 'member', NOW), 'numeric')
    assert.equal(tileLayout(s, 'admin', NOW), 'numeric')
  })

  it('游客永不进入 numeric', () => {
    // 后端对游客恒回 0，但即使某天回了真数字，游客也不该看到未读数字墙
    const s = makeSource({ unread_count: 400, recent_items: makePreviews(8) })
    assert.equal(tileLayout(s, 'guest', NOW), 'list')
  })

  it('未读 19 还不够，20 才够', () => {
    const items = makePreviews(8)
    assert.equal(
      tileLayout(makeSource({ unread_count: 19, recent_items: items }), 'member', NOW),
      'list',
    )
    assert.equal(
      tileLayout(makeSource({ unread_count: 20, recent_items: items }), 'member', NOW),
      'numeric',
    )
  })

  it('沉寂 > 60 天且 pulses ≥ 6 → cadence', () => {
    const s = makeSource({
      last_success_at: daysAgo(210),
      recent_items: makePreviews(8, { published_at: daysAgo(210) }),
      pulses: [210, 240, 280, 320, 400, 520],
    })
    assert.equal(tileLayout(s, 'guest', NOW), 'cadence')
  })

  it('沉寂但 pulses 不足 6 根 → 降级 feature，不画只有三根线的节律图', () => {
    const s = makeSource({
      last_success_at: daysAgo(210),
      recent_items: makePreviews(8, { published_at: daysAgo(210) }),
      pulses: [210, 240, 300],
    })
    assert.equal(tileLayout(s, 'guest', NOW), 'feature')
  })

  it('pulses 缺失的沉寂源 → feature', () => {
    const s = makeSource({
      last_success_at: daysAgo(400),
      recent_items: makePreviews(8, { published_at: daysAgo(400) }),
    })
    assert.equal(tileLayout(s, 'guest', NOW), 'feature')
  })

  it('沉寂源即使条目很多也不掉进 list', () => {
    // list 会摆出一排几年前的标题，读不出任何东西
    const s = makeSource({
      last_success_at: daysAgo(400),
      recent_items: makePreviews(12, { published_at: daysAgo(400) }),
    })
    assert.equal(tileLayout(s, 'guest', NOW), 'feature')
  })

  it('没有任何时间戳时不走 cadence', () => {
    const s = makeSource({
      last_success_at: null,
      recent_items: makePreviews(8, { published_at: null }),
      pulses: [300, 340, 380, 420, 460, 500],
    })
    assert.equal(tileLayout(s, 'guest', NOW), 'list')
  })

  it('沉寂刚好 60 天不算沉寂（严格大于）', () => {
    const s = makeSource({
      last_success_at: daysAgo(60),
      recent_items: makePreviews(8, { published_at: daysAgo(60) }),
      pulses: [60, 90, 120, 150, 180, 210],
    })
    assert.equal(tileLayout(s, 'guest', NOW), 'list')
  })

  it('条目 ≤ 2 → feature；≥ 3 → list', () => {
    const two = makeSource({ recent_items: makePreviews(2) })
    const three = makeSource({ recent_items: makePreviews(3) })
    assert.equal(tileLayout(two, 'guest', NOW), 'feature')
    assert.equal(tileLayout(three, 'guest', NOW), 'list')
  })

  it('补拉后的条数覆盖 recent_items 长度', () => {
    // 后端预览只回 3 条，但网格补拉到 8 条 —— 不传 itemCount 会误判
    const s = makeSource({ recent_items: makePreviews(1) })
    assert.equal(tileLayout(s, 'guest', NOW), 'feature')
    assert.equal(tileLayout(s, 'guest', NOW, 8), 'list')
  })

  it('numeric 优先于 cadence（有未读就还有可看的）', () => {
    const s = makeSource({
      unread_count: 40,
      last_success_at: daysAgo(300),
      recent_items: makePreviews(8, { published_at: daysAgo(300) }),
      pulses: [300, 330, 360, 390, 420, 450],
    })
    assert.equal(tileLayout(s, 'member', NOW), 'numeric')
    assert.equal(tileLayout(s, 'guest', NOW), 'cadence')
  })
})

describe('downgradeForBand', () => {
  it('desktop 保留三档', () => {
    for (const size of BREW_TILE_SIZES) {
      assert.equal(downgradeForBand(size, 'desktop'), size)
    }
  })
  it('tablet / phone 没有 4x4', () => {
    for (const band of ['tablet', 'phone'] as const) {
      assert.equal(downgradeForBand('4x4', band), '4x2')
      assert.equal(downgradeForBand('4x2', band), '4x2')
      assert.equal(downgradeForBand('2x2', band), '2x2')
    }
  })

  it('竖条 / 横条各档都不降 —— 手机 4 列也放得下', () => {
    for (const band of ['tablet', 'phone'] as const) {
      assert.equal(downgradeForBand('1x2', band), '1x2')
      assert.equal(downgradeForBand('2x1', band), '2x1')
    }
  })
})

describe('tileSize', () => {
  // 3 条预览 = 正常的 list 源。空源会被「内容撑不起来」那条压到 4x2，
  // 那是另一个用例要验的事。
  const s = () => makeSource({ id: 1, recent_items: makePreviews(5) })

  it('源太少（< 6）一律撑满', () => {
    assert.equal(tileSize(0, s(), 'desktop', 1), '4x4')
    assert.equal(tileSize(0, s(), 'desktop', 5), '4x4')
    assert.equal(tileSize(0, s(), 'desktop', 6), '2x2')
  })

  it('源太少时窄屏仍要降档', () => {
    assert.equal(tileSize(0, s(), 'tablet', 3), '4x2')
    assert.equal(tileSize(0, s(), 'phone', 3), '4x2')
  })

  it('入口型来源固定竖条，不看分数', () => {
    const link = makeSource({ source_type: 'link' })
    assert.equal(tileSize(0.99, link, 'desktop', 20), '1x2')
    assert.equal(tileSize(0, link, 'desktop', 20), '1x2')
  })

  it('入口型来源不吃「源太少一律撑满」—— 三个友链不该各占 4x4', () => {
    const link = makeSource({ source_type: 'link' })
    assert.equal(tileSize(0, link, 'desktop', 3), '1x2')
    assert.equal(tileSize(0, link, 'phone', 1), '1x2')
  })

  it('入口型来源仍认用户锁定', () => {
    const bar = makeSource({ source_type: 'link', card_size: 'bar' })
    const mini = makeSource({ source_type: 'link', card_size: 'mini' })
    assert.equal(tileSize(0, bar, 'desktop', 20), '2x1')
    assert.equal(tileSize(0, mini, 'desktop', 20), '4x2')
  })

  it('分数分三档', () => {
    assert.equal(tileSize(0.55, s(), 'desktop', 20), '4x4')
    assert.equal(tileSize(0.54, s(), 'desktop', 20), '4x2')
    assert.equal(tileSize(0.3, s(), 'desktop', 20), '4x2')
    assert.equal(tileSize(0.29, s(), 'desktop', 20), '2x2')
  })

  it('窄屏没有 4x4', () => {
    for (const band of ['tablet', 'phone'] as const) {
      assert.equal(tileSize(0.9, s(), band, 20), '4x2')
    }
  })

  it('card_size 视为用户锁定，不再看分数', () => {
    const tiny = makeSource({ card_size: 'tiny' })
    const mini = makeSource({ card_size: 'mini' })
    const full = makeSource({ card_size: 'full' })
    assert.equal(tileSize(0.99, tiny, 'desktop', 20), '2x2')
    assert.equal(tileSize(0, mini, 'desktop', 20), '4x2')
    assert.equal(tileSize(0, full, 'desktop', 20), '4x4')
  })

  it('锁定尺寸也要套 band 降档', () => {
    const full = makeSource({ card_size: 'full' })
    assert.equal(tileSize(0, full, 'tablet', 20), '4x2')
  })

  it('内容撑不起来的源不给 4x4：无封面且不满一页列表 → 最多 4x2', () => {
    const thin = makeSource({ recent_items: makePreviews(2), error_count: 7 })
    assert.equal(tileSize(0.9, thin, 'desktop', 20), '4x2')
    // 三四条纯文字也撑不起 320px
    const few = makeSource({ recent_items: makePreviews(4) })
    assert.equal(tileSize(0.9, few, 'desktop', 20), '4x2')
    // ≤2 条 + 封面 = feature 通栏大图，撑得起 4x4
    const covered = makeSource({
      recent_items: makePreviews(2, { image: 'https://example.com/c.png' }),
    })
    assert.equal(tileSize(0.9, covered, 'desktop', 20), '4x4')
    // 三四条只配 52px 小方图，有封面也撑不起
    const fewCovered = makeSource({
      recent_items: makePreviews(4, { image: 'https://example.com/c.png' }),
    })
    assert.equal(tileSize(0.9, fewCovered, 'desktop', 20), '4x2')
    // 够铺满一页列表（5 条）也行
    const listy = makeSource({ recent_items: makePreviews(5) })
    assert.equal(tileSize(0.9, listy, 'desktop', 20), '4x4')
    // 「源太少一律撑满」不受这条影响
    assert.equal(tileSize(0.9, thin, 'desktop', 3), '4x4')
  })

  it('锁定优先于「源太少一律撑满」', () => {
    const tiny = makeSource({ card_size: 'tiny' })
    assert.equal(tileSize(0, tiny, 'desktop', 2), '2x2')
  })
})

describe('topicTileSize', () => {
  it('智能模式前 2 个 4x4，其余 4x2', () => {
    assert.equal(topicTileSize(0, 'smart', 'desktop'), '4x4')
    assert.equal(topicTileSize(1, 'smart', 'desktop'), '4x4')
    assert.equal(topicTileSize(2, 'smart', 'desktop'), '4x2')
  })
  it('主题模式前 4 个 4x4', () => {
    assert.equal(topicTileSize(3, 'topic', 'desktop'), '4x4')
    assert.equal(topicTileSize(4, 'topic', 'desktop'), '4x2')
  })
  it('主题卡永不进 2x2，窄屏降到 4x2', () => {
    for (const band of ['tablet', 'phone'] as const) {
      for (const i of [0, 1, 2, 9]) {
        assert.equal(topicTileSize(i, 'topic', band), '4x2')
      }
    }
  })
})

describe('尺寸锁', () => {
  it('card_size 与档位互为反向', () => {
    for (const size of BREW_TILE_SIZES) {
      const card = cardSizeForTile(size)
      assert.equal(lockedTileSize({ card_size: card }), size)
    }
  })

  it('没锁就是 null', () => {
    assert.equal(lockedTileSize({ card_size: null }), null)
  })

  it('认不出的 card_size 当没锁 —— 不要抛，也不要静默变成某一档', () => {
    assert.equal(
      lockedTileSize({ card_size: 'nope' as never }),
      null,
    )
  })

  it('入口型来源与内容源的可选档位不同', () => {
    assert.deepEqual(
      allowedTileSizes({ source_type: 'link' }),
      SITE_TILE_SIZES,
    )
    assert.deepEqual(
      allowedTileSizes({ source_type: 'rss' }),
      CONTENT_TILE_SIZES,
    )
  })

  it('内容源拿不到竖条 / 横条', () => {
    const content = allowedTileSizes({ source_type: 'rss' })
    assert.equal(content.includes('1x2'), false)
    assert.equal(content.includes('2x1'), false)
  })

  it('轮转：未锁 → 逐档 → 回到未锁', () => {
    assert.equal(nextLockedSize(null, SITE_TILE_SIZES), '1x2')
    assert.equal(nextLockedSize('1x2', SITE_TILE_SIZES), '2x1')
    assert.equal(nextLockedSize('2x1', SITE_TILE_SIZES), '2x2')
    assert.equal(nextLockedSize('2x2', SITE_TILE_SIZES), '4x2')
    assert.equal(nextLockedSize('4x2', SITE_TILE_SIZES), null)
  })

  it('轮转：当前档不在列表里就从头开始，用户点得回未锁定', () => {
    assert.equal(nextLockedSize('1x2', CONTENT_TILE_SIZES), '2x2')
  })

  it('空列表恒为未锁', () => {
    assert.equal(nextLockedSize('2x2', []), null)
  })
})
