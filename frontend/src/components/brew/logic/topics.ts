/**
 * 跨源主题聚类。
 *
 * 「智能」的真正产出是这个，不是排序 —— 排序只是把源换个顺序，聚类才让
 * 不同源的同题文章第一次出现在同一张卡里。
 *
 * 两条纪律：
 * - `clusterTopics` **只看 `item.topic`**，不管这个标签是关键词还是 AI 写的。
 *   关键词版与 AI 版的替换点只在「写入 topic」那一步。
 * - 主题 key 固定 10 个，AI 只做分类不许自创。分不进去的文章 topic 保持 null，
 *   只留在源磁贴里 —— 不建「其他」桶。
 */

import type { BrewItem } from '../../../types/brew'

/**
 * 聚类需要的最小条目形状。
 *
 * `BrewItem`（文章列表）与「源预览 + 源信息」两条数据源都能满足它 ——
 * 主题卡不需要正文，也就不该为它多开一个接口。
 */
export interface TopicItem {
  id: number
  title: string
  image: string | null
  published_at: number | null
  /** 预定义主题 key；null / 缺失不参与聚类 */
  topic?: string | null
  /** 用于「N 个源」统计 */
  source_id: number
  source_name?: string | null
}

export interface BrewTopic {
  /** 稳定 key，如 "engineering"。与后端词表必须一致。 */
  key: string
  /** i18n key（t.brew 下），如 "topicEngineering"。不是展示文案本身。 */
  nameKey: string
  /** 身份色，给 Glow / 字标 */
  hue: string
  /** 该主题下的文章，已按发布时间新→旧 */
  items: TopicItem[]
}

/** 不足这个篇数不成卡。 */
export const TOPIC_MIN_ITEMS = 3
/** 聚类窗口（天）。只看近 30 天，主题墙才有「现在有什么可看」的意思。 */
export const TOPIC_WINDOW_DAYS = 30

const MS_PER_DAY = 86_400_000

interface TopicDef {
  key: string
  nameKey: string
  hue: string
  /** 标题 / 摘要关键词种子，大小写不敏感 */
  keywords: readonly string[]
}

/**
 * 预定义主题。key 稳定、文案走 i18n。
 *
 * 词表后端另有一份（入库同步打标用），**key 必须一致**。
 * 宁可漏标（null），不要标错硬塞。
 */
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

/** 主题的 i18n key（t.brew 下）；未知 key 返回 null。 */
export function topicNameKey(key: string): string | null {
  return TOPIC_BY_KEY.get(key)?.nameKey ?? null
}

/** 主题身份色；未知 key 返回 null。 */
export function topicHue(key: string): string | null {
  return TOPIC_BY_KEY.get(key)?.hue ?? null
}

/** 摘要用于匹配的前缀长度（与后端 prompt 的「摘要前 200 字」一致）。 */
const SUMMARY_MATCH_CHARS = 200

/**
 * 关键词打标。命中多个主题时按 `TOPIC_DEFS` 顺序取第一个 —— 单标签、确定性。
 * 一个都不命中返回 null（不是「其他」）。
 */
export function inferTopicByKeywords(
  item: Pick<BrewItem, 'title' | 'summary'>,
): string | null {
  const title = (item.title || '').toLowerCase()
  const summary = (item.summary || '')
    .replace(/<[^>]*>/g, '')
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

/**
 * 按 `item.topic` 聚类。
 *
 * - 只收窗口内（近 `TOPIC_WINDOW_DAYS` 天）且 topic 在预定义表内的文章
 * - 没有 `published_at` 的文章无法判断是否在窗口内，跳过
 * - 不足 `TOPIC_MIN_ITEMS` 篇的主题不成卡
 * - 主题按篇数降序，同篇数按预定义顺序（稳定，不随刷新抖动）
 */
export function clusterTopics(
  items: readonly TopicItem[],
  now: number,
): BrewTopic[] {
  const cutoff = now - TOPIC_WINDOW_DAYS * MS_PER_DAY
  const buckets = new Map<string, TopicItem[]>()

  for (const item of items) {
    const key = item.topic
    if (!key || !TOPIC_BY_KEY.has(key)) continue
    const at = item.published_at
    if (typeof at !== 'number' || at <= 0) continue
    if (at < cutoff) continue

    const bucket = buckets.get(key)
    if (bucket) bucket.push(item)
    else buckets.set(key, [item])
  }

  const orderOf = new Map(TOPIC_DEFS.map((d, i) => [d.key, i]))

  return [...buckets.entries()]
    .filter(([, list]) => list.length >= TOPIC_MIN_ITEMS)
    .map(([key, list]) => {
      const def = TOPIC_BY_KEY.get(key)!
      return {
        key,
        nameKey: def.nameKey,
        hue: def.hue,
        items: [...list].sort(
          (a, b) => (b.published_at ?? 0) - (a.published_at ?? 0),
        ),
      }
    })
    .sort((a, b) => {
      if (b.items.length !== a.items.length) return b.items.length - a.items.length
      return (orderOf.get(a.key) ?? 0) - (orderOf.get(b.key) ?? 0)
    })
}

/**
 * 把源列表里的预览摊平成 `TopicItem[]`（补上 source_id / source_name）。
 *
 * 首页磁贴靠这一步只用 `getSources()` 就能聚类 —— 不为主题卡新开接口。
 */
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

/** 这个主题覆盖了多少个源（主题卡上的「N 个源」）。 */
export function topicSourceCount(topic: BrewTopic): number {
  return new Set(topic.items.map((i) => i.source_id)).size
}
