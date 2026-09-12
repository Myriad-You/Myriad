/** clusterTopics 只看 item.topic。主题 key 固定 10 个，分不进去保持 null，不建「其他」。 */

import type { BrewItem } from '../../../types/brew'

/** 主题卡不另开接口。 */
export interface TopicItem {
  id: number
  title: string
  image: string | null
  published_at: number | null
  /** null / 缺失不参与聚类 */
  topic?: string | null
  source_id: number
  source_name?: string | null
}

export type TopicNameKey =
  | 'topicEngineering'
  | 'topicSystems'
  | 'topicAi'
  | 'topicProduct'
  | 'topicWriting'
  | 'topicTools'
  | 'topicCulture'
  | 'topicSecurity'
  | 'topicOss'
  | 'topicHardware'

export interface BrewTopic {
  /** 与后端词表必须一致。 */
  key: string
  /** i18n key，不是展示文案。 */
  nameKey: TopicNameKey
  hue: string
  items: TopicItem[]
}

export const TOPIC_MIN_ITEMS = 3
export const TOPIC_WINDOW_DAYS = 30

const MS_PER_DAY = 86_400_000

interface TopicDef {
  key: string
  nameKey: TopicNameKey
  hue: string
  keywords: readonly string[]
}

/** 前后端 key 必须一致。宁可漏标，不要硬塞。 */
const TOPIC_DEFS: readonly TopicDef[] = [
  {
    key: 'engineering',
    nameKey: 'topicEngineering',
    hue: '#6366f1',
    keywords: [
      'refactor',
      '重构',
      '微服务',
      '单体',
      'code review',
      '代码审查',
      'rust',
      'typescript',
    ],
  },
  {
    key: 'systems',
    nameKey: 'topicSystems',
    hue: '#0ea5e9',
    keywords: [
      'linux',
      'kernel',
      'tcp',
      'dns',
      'sqlite',
      'postgres',
      '性能',
      'perf',
      'jit',
    ],
  },
  {
    key: 'ai',
    nameKey: 'topicAi',
    hue: '#8b5cf6',
    keywords: [
      'llm',
      'gpt',
      '模型',
      'transformer',
      'agent',
      'embedding',
      '提示词',
    ],
  },
  {
    key: 'product',
    nameKey: 'topicProduct',
    hue: '#f97316',
    keywords: ['设计', 'ux', 'ui', '独立开发', '产品', '交互'],
  },
  {
    key: 'writing',
    nameKey: 'topicWriting',
    hue: '#d97706',
    keywords: ['中文', '排版', '写作', '播客', '字体'],
  },
  {
    key: 'tools',
    nameKey: 'topicTools',
    hue: '#14b8a6',
    keywords: ['效率', '笔记', '工作流', '周刊', '工具'],
  },
  {
    key: 'culture',
    nameKey: 'topicCulture',
    hue: '#ec4899',
    keywords: ['生活', '文化', '旅行', '城市'],
  },
  {
    key: 'security',
    nameKey: 'topicSecurity',
    hue: '#ef4444',
    keywords: ['安全', '漏洞', 'cve', '加密', 'privacy'],
  },
  {
    key: 'oss',
    nameKey: 'topicOss',
    hue: '#22c55e',
    keywords: ['开源', 'github', 'license', '社区'],
  },
  {
    key: 'hardware',
    nameKey: 'topicHardware',
    hue: '#64748b',
    keywords: ['芯片', '硬件', 'pcb', '制造', 'risc-v'],
  },
] as const

export const PREDEFINED_TOPICS: readonly string[] = TOPIC_DEFS.map((d) => d.key)

const TOPIC_BY_KEY = new Map(TOPIC_DEFS.map((d) => [d.key, d]))

export function isPredefinedTopic(key: string | null | undefined): boolean {
  return Boolean(key && TOPIC_BY_KEY.has(key))
}

export function topicNameKey(key: string): TopicNameKey | null {
  return TOPIC_BY_KEY.get(key)?.nameKey ?? null
}

export function topicDisplayName(
  topic: { key: string; nameKey: TopicNameKey },
  labels: Partial<Record<TopicNameKey, string>>,
): string {
  const value = labels[topic.nameKey]
  return typeof value === 'string' && value.length > 0 ? value : topic.key
}

export function topicHue(key: string): string | null {
  return TOPIC_BY_KEY.get(key)?.hue ?? null
}

const SUMMARY_MATCH_CHARS = 200

/** 多命中取 TOPIC_DEFS 第一项。都不命中返回 null，不是「其他」。 */
export function inferTopicByKeywords(
  item: Pick<BrewItem, 'title' | 'summary'>,
): string | null {
  const title = (item.title || '').toLowerCase()
  const summary = (item.summary || '')
    .replaceAll(/<[^>]*>/g, '')
    .slice(0, SUMMARY_MATCH_CHARS)
    .toLowerCase()
  const haystack = `${title}\n${summary}`
  if (!haystack.trim()) return null

  for (const def of TOPIC_DEFS) {
    for (const kw of def.keywords) {
      if (haystack.includes(kw)) return def.key
    }
  }
  return null
}

/** 只收窗口内且 topic 在表内的文章。无 published_at 则跳过。不足 TOPIC_MIN_ITEMS 不成卡。 */
export function clusterTopics(
  items: readonly TopicItem[],
  now: number,
): BrewTopic[] {
  const cutoff = now - TOPIC_WINDOW_DAYS * MS_PER_DAY
  const eligible = items.filter((item) => {
    const key = item.topic
    if (!key || !TOPIC_BY_KEY.has(key)) return false
    const at = item.published_at
    return typeof at === 'number' && at > 0 && at >= cutoff
  })
  const buckets = Map.groupBy(eligible, (item) => item.topic as string)

  const orderOf = new Map(TOPIC_DEFS.map((d, i) => [d.key, i]))

  return Iterator.from(buckets.entries()).toArray()
    .filter(([, list]) => list.length >= TOPIC_MIN_ITEMS)
    .map(([key, list]) => {
      const def = TOPIC_BY_KEY.get(key)!
      return {
        key,
        nameKey: def.nameKey,
        hue: def.hue,
        items: list.toSorted(
          (a, b) => (b.published_at ?? 0) - (a.published_at ?? 0),
        ),
      }
    })
    .toSorted((a, b) => {
      if (b.items.length !== a.items.length) return b.items.length - a.items.length
      return (orderOf.get(a.key) ?? 0) - (orderOf.get(b.key) ?? 0)
    })
}

/** 不为主题卡新开接口。 */
export function previewsToTopicItems(
  sources: readonly {
    id: number
    name: string
    recent_items?: {
      id: number
      title: string
      image: string | null
      published_at: number | null
      topic?: string | null
    }[]
  }[],
): TopicItem[] {
  const out: TopicItem[] = []
  for (const s of sources) {
    for (const p of s.recent_items ?? []) {
      out.push({
        id: p.id,
        title: p.title,
        image: p.image,
        published_at: p.published_at,
        topic: p.topic ?? null,
        source_id: s.id,
        source_name: s.name,
      })
    }
  }
  return out
}

export function topicSourceCount(topic: BrewTopic): number {
  return new Set(topic.items.map((i) => i.source_id)).size
}
