import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { daysAgo, makeSource, NOW } from './fixtures.ts'
import {
  brewScore,
  bucketScore,
  compareByScore,
  daysSinceLastPublish,
  lastPublishAt,
  roleFromAuth,
  SCORE_BUCKET,
  SCORE_WEIGHTS,
  scoreFactors,
  sortByScore,
} from './score.ts'

describe('roleFromAuth', () => {
  it('三档角色', () => {
    assert.equal(roleFromAuth(false, false), 'guest')
    assert.equal(roleFromAuth(true, false), 'member')
    assert.equal(roleFromAuth(true, true), 'admin')
    // isAdmin 已隐含登录，漏传 isAuthenticated 也不降成游客。
    assert.equal(roleFromAuth(false, true), 'admin')
  })
})

describe('lastPublishAt', () => {
  it('优先最新一篇的发布时间，而不是抓取时刻', () => {
    const s = makeSource({
      last_success_at: NOW,
      recent_items: [{ id: 1, title: 'a', summary: null, image: null, published_at: daysAgo(30), is_read: false }],
    })
    assert.equal(lastPublishAt(s), daysAgo(30))
  })
  it('没有条目时回落 last_success_at', () => {
    const s = makeSource({ last_success_at: daysAgo(5), recent_items: [] })
    assert.equal(lastPublishAt(s), daysAgo(5))
  })
  it('两者都没有时返回 null', () => {
    const s = makeSource({ last_success_at: null, recent_items: [] })
    assert.equal(lastPublishAt(s), null)
    assert.equal(daysSinceLastPublish(s, NOW), null)
  })
})

describe('scoreFactors', () => {
  it('own: category 含「我」且非 admin_only', () => {
    assert.equal(scoreFactors(makeSource({ category: '我' }), NOW).own, 1)
    assert.equal(scoreFactors(makeSource({ category: '博客,我' }), NOW).own, 1)
    assert.equal(scoreFactors(makeSource({ category: '博客' }), NOW).own, 0)
    // admin_only 源不算公开自有内容。
    assert.equal(
      scoreFactors(makeSource({ category: '我', admin_only: true }), NOW).own,
      0,
    )
  })

  it('recency: exp(-days / 21)，21 天恰好 1/e', () => {
    const fresh = scoreFactors(makeSource({ last_success_at: NOW }), NOW).recency
    assert.ok(Math.abs(fresh - 1) < 1e-9, `今天发的应当接近 1，实际 ${fresh}`)

    const tau = scoreFactors(
      makeSource({ last_success_at: daysAgo(21) }),
      NOW,
    ).recency
    assert.ok(Math.abs(tau - Math.E ** -1) < 1e-6, `21 天应为 1/e，实际 ${tau}`)

    const stale = scoreFactors(
      makeSource({ last_success_at: daysAgo(200) }),
      NOW,
    ).recency
    assert.ok(stale < 0.001, `200 天应几乎归零，实际 ${stale}`)
  })

  it('recency: 友链恒为 0（没有更新时间这回事）', () => {
    const link = makeSource({ source_type: 'link', last_success_at: NOW })
    assert.equal(scoreFactors(link, NOW).recency, 0)
  })

  it('recency: 没有任何时间戳时为 0，不是 1', () => {
    const s = makeSource({ last_success_at: null, recent_items: [] })
    assert.equal(scoreFactors(s, NOW).recency, 0)
  })

  it('corpus: log1p 归一，饱和后夹到 1', () => {
    assert.equal(scoreFactors(makeSource({ item_count: 0 }), NOW).corpus, 0)
    const mid = scoreFactors(makeSource({ item_count: 44 }), NOW).corpus
    assert.ok(mid > 0.4 && mid < 0.6, `44 篇应在中段，实际 ${mid}`)
    assert.equal(scoreFactors(makeSource({ item_count: 2000 }), NOW).corpus, 1)
    assert.equal(scoreFactors(makeSource({ item_count: 99999 }), NOW).corpus, 1)
  })

  it('unread: log1p 归一，50 篇饱和', () => {
    assert.equal(scoreFactors(makeSource({ unread_count: 0 }), NOW).unread, 0)
    assert.equal(scoreFactors(makeSource({ unread_count: 50 }), NOW).unread, 1)
    assert.equal(scoreFactors(makeSource({ unread_count: 500 }), NOW).unread, 1)
  })

  it('pin: sort_order 非空即 1', () => {
    assert.equal(scoreFactors(makeSource({ sort_order: null }), NOW).pin, 0)
    assert.equal(scoreFactors(makeSource({ sort_order: 0 }), NOW).pin, 1)
    assert.equal(scoreFactors(makeSource({ sort_order: 7 }), NOW).pin, 1)
  })

  it('fail: error_count > 0 即 1', () => {
    assert.equal(scoreFactors(makeSource({ error_count: 0 }), NOW).fail, 0)
    assert.equal(scoreFactors(makeSource({ error_count: 1 }), NOW).fail, 1)
  })
})

describe('SCORE_WEIGHTS', () => {
  it('游客 unread 权重必须为 0', () => {
    // 游客 unread 权重必须为 0。
    assert.equal(SCORE_WEIGHTS.guest.unread, 0)
  })

  it('fail 只对管理员为正', () => {
    assert.ok(SCORE_WEIGHTS.guest.fail < 0)
    assert.ok(SCORE_WEIGHTS.member.fail < 0)
    assert.ok(SCORE_WEIGHTS.admin.fail > 0)
  })

  it('游客最看重自有内容，成员最看重未读', () => {
    const g = SCORE_WEIGHTS.guest
    const m = SCORE_WEIGHTS.member
    assert.ok(g.own > g.recency && g.own > g.corpus)
    assert.ok(m.unread > m.own && m.unread > m.recency)
  })
})

describe('brewScore', () => {
  it('游客侧未读不影响分数', () => {
    const base = makeSource({ id: 1, unread_count: 0 })
    const loaded = makeSource({ id: 1, unread_count: 47 })
    assert.equal(brewScore(base, 'guest', NOW), brewScore(loaded, 'guest', NOW))
    assert.ok(brewScore(loaded, 'member', NOW) > brewScore(base, 'member', NOW))
  })

  it('失败源：游客沉底、管理员抬头', () => {
    const ok = makeSource({ id: 1, error_count: 0 })
    const broken = makeSource({ id: 2, error_count: 3 })
    assert.ok(brewScore(broken, 'guest', NOW) < brewScore(ok, 'guest', NOW))
    assert.ok(brewScore(broken, 'admin', NOW) > brewScore(ok, 'admin', NOW))
  })
})

describe('bucketScore', () => {
  it('落到 0.05 的整数倍', () => {
    assert.ok(Math.abs(bucketScore(0.51) - 0.5) < 1e-9)
    assert.ok(Math.abs(bucketScore(0.53) - 0.55) < 1e-9)
    assert.ok(Math.abs(bucketScore(0.5) - 0.5) < 1e-9)
    assert.equal(SCORE_BUCKET, 0.05)
  })
})

describe('compareByScore / sortByScore', () => {
  const cohort = () => [
    makeSource({ id: 10, name: '我的博客', category: '我', item_count: 120, last_success_at: daysAgo(3) }),
    makeSource({ id: 20, name: '高未读', item_count: 800, unread_count: 47, last_success_at: daysAgo(2) }),
    makeSource({ id: 30, name: '大语料', item_count: 1900, last_success_at: daysAgo(40) }),
    makeSource({ id: 40, name: '失败源', item_count: 300, error_count: 5, unread_count: 30, last_success_at: daysAgo(1) }),
    makeSource({ id: 50, name: '平凡源', item_count: 60, last_success_at: daysAgo(9) }),
    makeSource({ id: 60, name: '友链', source_type: 'link', item_count: 0, last_success_at: NOW }),
  ]

  it('同档内按 id 升序（未读 +1 不挪卡）', () => {
    const a = makeSource({ id: 9, item_count: 100, last_success_at: daysAgo(4) })
    const b = makeSource({ id: 3, item_count: 100, last_success_at: daysAgo(4) })
    assert.deepEqual(
      sortByScore([a, b], 'guest', NOW).map((s) => s.id),
      [3, 9],
    )
  })

  it('档内微小变化不改变顺序', () => {
    const before = sortByScore(cohort(), 'member', NOW).map((s) => s.id)
    const nudged = cohort().map((s) =>
      s.id === 20 ? { ...s, unread_count: s.unread_count + 1 } : s,
    )
    assert.deepEqual(sortByScore(nudged, 'member', NOW).map((s) => s.id), before)
  })

  it('游客：自有源排在最前', () => {
    const order = sortByScore(cohort(), 'guest', NOW).map((s) => s.id)
    assert.equal(order[0], 10, `游客首位应是自有源，实际顺序 ${order}`)
  })

  it('成员：高未读源比同龄平凡源靠前', () => {
    const order = sortByScore(cohort(), 'member', NOW).map((s) => s.id)
    assert.ok(order.indexOf(20) < order.indexOf(50))
  })

  it('失败源只在管理员视图里抬头', () => {
    const guest = sortByScore(cohort(), 'guest', NOW).map((s) => s.id)
    const admin = sortByScore(cohort(), 'admin', NOW).map((s) => s.id)
    assert.ok(
      admin.indexOf(40) < guest.indexOf(40),
      `失败源应在 admin 视图更靠前：guest=${guest} admin=${admin}`,
    )
    assert.ok(guest.indexOf(40) > 2, `失败源在游客视图应沉底，实际 ${guest}`)
  })

  it('三种角色的头部顺序互不相同', () => {
    const head = (role: 'guest' | 'member' | 'admin') =>
      sortByScore(cohort(), role, NOW)
        .slice(0, 3)
        .map((s) => s.id)
        .join(',')
    const g = head('guest')
    const m = head('member')
    const a = head('admin')
    assert.notEqual(g, m)
    assert.notEqual(m, a)
  })

  it('比较器自身稳定：a<b 蕴含 b>a', () => {
    const list = cohort()
    for (const x of list) {
      for (const y of list) {
        if (x.id === y.id) continue
        const ab = compareByScore(x, y, 'member', NOW)
        const ba = compareByScore(y, x, 'member', NOW)
        assert.equal(Math.sign(ab), -Math.sign(ba))
      }
    }
  })

  it('不改入参数组', () => {
    const list = cohort()
    const ids = list.map((s) => s.id)
    sortByScore(list, 'guest', NOW)
    assert.deepEqual(list.map((s) => s.id), ids)
  })
})
