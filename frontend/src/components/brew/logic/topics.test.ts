/**
 * 主题聚类与关键词打标的单元测试。
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/brew/logic/topics.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { daysAgo, makeItem, NOW } from './fixtures.ts'
import {
  clusterTopics,
  inferTopicByKeywords,
  isPredefinedTopic,
  PREDEFINED_TOPICS,
  TOPIC_MIN_ITEMS,
  TOPIC_WINDOW_DAYS,
  topicHue,
  topicNameKey,
  topicSourceCount,
} from './topics.ts'

describe('PREDEFINED_TOPICS', () => {
  it('固定 10 个 key，且没有「其他」桶', () => {
    assert.equal(PREDEFINED_TOPICS.length, 10)
    assert.deepEqual([...PREDEFINED_TOPICS], [
      'engineering',
      'systems',
      'ai',
      'product',
      'writing',
      'tools',
      'culture',
      'security',
      'oss',
      'hardware',
    ])
    assert.equal(PREDEFINED_TOPICS.includes('other'), false)
    assert.equal(PREDEFINED_TOPICS.includes('misc'), false)
  })

  it('每个 key 都有 i18n key 与身份色', () => {
    for (const key of PREDEFINED_TOPICS) {
      assert.ok(topicNameKey(key), `${key} 缺 nameKey`)
      assert.match(topicHue(key)!, /^#[0-9a-f]{6}$/i, `${key} 的 hue 不是 hex`)
      assert.equal(isPredefinedTopic(key), true)
    }
    assert.equal(isPredefinedTopic('unknown'), false)
    assert.equal(isPredefinedTopic(null), false)
    assert.equal(topicNameKey('unknown'), null)
  })

  it('nameKey 是 i18n key 而不是展示文案', () => {
    // 主题名走 i18n、跟界面语言；写死中文会让 en-US / ja-JP 露出中文
    for (const key of PREDEFINED_TOPICS) {
      assert.match(topicNameKey(key)!, /^topic[A-Z]/)
    }
  })
})

describe('inferTopicByKeywords', () => {
  it('标题命中', () => {
    assert.equal(
      inferTopicByKeywords({ title: '这次重构把单体拆了', summary: null }),
      'engineering',
    )
    assert.equal(
      inferTopicByKeywords({ title: 'SQLite 的 WAL 到底怎么工作', summary: null }),
      'systems',
    )
  })

  it('摘要命中；HTML 标签不干扰', () => {
    assert.equal(
      inferTopicByKeywords({
        title: '周末随笔',
        summary: '<p>聊聊 <b>embedding</b> 的取舍</p>',
      }),
      'ai',
    )
  })

  it('大小写不敏感', () => {
    assert.equal(
      inferTopicByKeywords({ title: 'Understanding TypeScript', summary: null }),
      'engineering',
    )
    assert.equal(inferTopicByKeywords({ title: 'CVE 复盘', summary: null }), 'security')
  })

  it('一个都不命中返回 null，不硬塞', () => {
    assert.equal(inferTopicByKeywords({ title: '今天天气不错', summary: null }), null)
    assert.equal(inferTopicByKeywords({ title: '', summary: null }), null)
    assert.equal(inferTopicByKeywords({ title: '   ', summary: '' }), null)
  })

  it('多主题命中时确定性取第一个（单标签）', () => {
    const item = { title: 'Rust 写的 kernel 模块', summary: null }
    const first = inferTopicByKeywords(item)
    assert.equal(first, 'engineering', '按预定义顺序，engineering 在 systems 之前')
    // 同一输入永远同一输出
    assert.equal(inferTopicByKeywords(item), first)
  })

  it('只看摘要前 200 字（与后端 prompt 一致）', () => {
    const far = { title: '无关标题', summary: `${'啊'.repeat(400)}kernel` }
    assert.equal(inferTopicByKeywords(far), null)
  })
})

describe('clusterTopics', () => {
  const eng = (over = {}) =>
    makeItem({ topic: 'engineering', published_at: daysAgo(3), ...over })

  it('不足 3 篇不成卡', () => {
    const two = [eng({ id: 1 }), eng({ id: 2 })]
    assert.deepEqual(clusterTopics(two, NOW), [])

    const three = [...two, eng({ id: 3 })]
    const out = clusterTopics(three, NOW)
    assert.equal(out.length, 1)
    assert.equal(out[0].key, 'engineering')
    assert.equal(out[0].items.length, TOPIC_MIN_ITEMS)
  })

  it('窗口外的文章不算', () => {
    const items = [
      eng({ id: 1, published_at: daysAgo(1) }),
      eng({ id: 2, published_at: daysAgo(2) }),
      eng({ id: 3, published_at: daysAgo(TOPIC_WINDOW_DAYS + 1) }),
    ]
    assert.deepEqual(clusterTopics(items, NOW), [], '只剩 2 篇在窗口内')
  })

  it('topic 为 null / 未知 key 的文章不参与', () => {
    const items = [
      makeItem({ id: 1, topic: null }),
      makeItem({ id: 2, topic: undefined }),
      makeItem({ id: 3, topic: 'other' }),
      makeItem({ id: 4, topic: '' }),
    ]
    assert.deepEqual(clusterTopics(items, NOW), [])
  })

  it('没有 published_at 的文章跳过（无法判断是否在窗口内）', () => {
    const items = [
      eng({ id: 1 }),
      eng({ id: 2 }),
      eng({ id: 3, published_at: null }),
    ]
    assert.deepEqual(clusterTopics(items, NOW), [])
  })

  it('组内按发布时间新→旧', () => {
    const items = [
      eng({ id: 1, published_at: daysAgo(9) }),
      eng({ id: 2, published_at: daysAgo(1) }),
      eng({ id: 3, published_at: daysAgo(5) }),
    ]
    const [topic] = clusterTopics(items, NOW)
    assert.deepEqual(topic.items.map((i) => i.id), [2, 3, 1])
  })

  it('主题按篇数降序，同篇数按预定义顺序（稳定）', () => {
    const items = [
      ...Array.from({ length: 5 }, (_, i) =>
        makeItem({ id: 100 + i, topic: 'ai', published_at: daysAgo(i + 1) }),
      ),
      ...Array.from({ length: 3 }, (_, i) =>
        makeItem({ id: 200 + i, topic: 'systems', published_at: daysAgo(i + 1) }),
      ),
      ...Array.from({ length: 3 }, (_, i) =>
        makeItem({ id: 300 + i, topic: 'engineering', published_at: daysAgo(i + 1) }),
      ),
    ]
    const keys = clusterTopics(items, NOW).map((t) => t.key)
    assert.deepEqual(keys, ['ai', 'engineering', 'systems'])
    // 打乱输入顺序也是同一个结果
    assert.deepEqual(clusterTopics([...items].reverse(), NOW).map((t) => t.key), keys)
  })

  it('聚类只看 item.topic，不重新跑关键词', () => {
    // 标题里全是 systems 的词，但已标 ai —— 打标是离线的唯一真相
    const items = Array.from({ length: 3 }, (_, i) =>
      makeItem({
        id: i + 1,
        topic: 'ai',
        title: 'linux kernel tcp dns',
        published_at: daysAgo(i + 1),
      }),
    )
    const out = clusterTopics(items, NOW)
    assert.equal(out.length, 1)
    assert.equal(out[0].key, 'ai')
  })

  it('携带 hue / nameKey，供 Glow 与字标使用', () => {
    const items = Array.from({ length: 3 }, (_, i) =>
      makeItem({ id: i + 1, topic: 'security', published_at: daysAgo(i + 1) }),
    )
    const [topic] = clusterTopics(items, NOW)
    assert.equal(topic.nameKey, topicNameKey('security'))
    assert.equal(topic.hue, topicHue('security'))
  })

  it('不改入参数组', () => {
    const items = [eng({ id: 1 }), eng({ id: 2 }), eng({ id: 3 })]
    const ids = items.map((i) => i.id)
    clusterTopics(items, NOW)
    assert.deepEqual(items.map((i) => i.id), ids)
  })
})

describe('topicSourceCount', () => {
  it('去重后的源数', () => {
    const items = [
      makeItem({ id: 1, source_id: 7, topic: 'oss', published_at: daysAgo(1) }),
      makeItem({ id: 2, source_id: 7, topic: 'oss', published_at: daysAgo(2) }),
      makeItem({ id: 3, source_id: 9, topic: 'oss', published_at: daysAgo(3) }),
    ]
    const [topic] = clusterTopics(items, NOW)
    assert.equal(topicSourceCount(topic), 2)
  })
})
