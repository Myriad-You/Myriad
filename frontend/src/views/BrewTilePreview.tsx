/** DEV-only tile sweep; no visual-regression tests. Fixtures pin to NOW. */

import type { BrewTileLayout, BrewTileSize } from '../components/brew/logic/layout'
import type { BrewViewerRole } from '../components/brew/logic/score'
import type { BrewTopic } from '../components/brew/logic/topics'
import type { BrewItemPreview, BrewSource } from '../types/brew'

import { useMemo, useState } from 'react'

import { allowedTileSizes } from '../components/brew/logic/layout'
import { clusterTopics } from '../components/brew/logic/topics'
import { BrewFeaturedTile } from '../components/brew/tiles/BrewFeaturedTile'
import { BrewSourceTile } from '../components/brew/tiles/BrewSourceTile'
import { BrewTopicTile } from '../components/brew/tiles/BrewTopicTile'

const MS_PER_DAY = 86_400_000
/** Same NOW as logic/fixtures. */
const NOW = Date.UTC(2025, 5, 15)
const daysAgo = (d: number) => NOW - d * MS_PER_DAY

const CELL = 82
const SIZE_PX: Record<BrewTileSize, { width: number; height: number }> = {
  '2x1': { width: CELL * 2, height: CELL },
  '2x2': { width: CELL * 2, height: CELL * 2 },
  '4x2': { width: CELL * 4, height: CELL * 2 },
  '4x4': { width: CELL * 4, height: CELL * 4 },
}

let seq = 0
function preview(over: Partial<BrewItemPreview> = {}): BrewItemPreview {
  seq += 1
  return {
    id: 9000 + seq,
    title: `一篇标题不长不短的文章，用来验证 clamp 与截断 ${seq}`,
    summary:
      '摘要用来验证无封面时的纯文本布局。这里刻意写长一点，看三行 clamp 之后有没有把行高压塌。',
    image: null,
    published_at: daysAgo(seq),
    is_read: false,
    ...over,
  }
}

function source(over: Partial<BrewSource> = {}): BrewSource {
  return {
    id: 1,
    user_id: 1,
    name: '示例订阅源',
    url: 'https://example.com/feed.xml',
    feed_type: 'rss',
    source_type: 'rss',
    category: null,
    icon: null,
    description: '一个用于验证排版的示例站点，简介刻意写到两三行。',
    site_url: 'https://example.com',
    update_interval: 3600,
    last_fetched_at: daysAgo(1),
    last_success_at: daysAgo(1),
    last_error: null,
    error_count: 0,
    enabled: true,
    item_count: 420,
    unread_count: 0,
    card_size: null,
    theme_color: '#f97316',
    sort_order: null,
    ai_style_tags: ['长文', '技术'],
    rsshub_route: null,
    admin_only: false,
    created_at: daysAgo(900),
    recent_items: [],
    ...over,
  }
}

/** 1×1 PNG is a soft-fail; SVG data URI counts as a real cover. */
const COVER = `data:image/svg+xml;utf8,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180">' +
    '<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">' +
    '<stop offset="0" stop-color="#f97316"/><stop offset="1" stop-color="#6366f1"/>' +
    '</linearGradient></defs><rect width="320" height="180" fill="url(#g)"/></svg>',
)}`

interface Case {
  label: string
  layout?: BrewTileLayout
  src: BrewSource
  items?: BrewItemPreview[]
}

const CASES: Case[] = [
  {
    label: 'feature · 有封面',
    layout: 'feature',
    src: source({ id: 11, name: '有封面的源' }),
    items: [preview({ image: COVER }), preview()],
  },
  {
    label: 'feature · 无封面（纯文本，不画灰块）',
    layout: 'feature',
    src: source({ id: 12, name: '无图的源', theme_color: '#0ea5e9' }),
    items: [preview(), preview()],
  },
  {
    label: 'feature · 无条目（退回站点简介）',
    layout: 'feature',
    src: source({ id: 13, name: '空源', theme_color: '#22c55e' }),
    items: [],
  },
  {
    label: 'feature · 无条目且无简介',
    layout: 'feature',
    src: source({ id: 14, name: '什么都没有', description: null }),
    items: [],
  },
  {
    label: 'list · 8 条（轮播）',
    layout: 'list',
    src: source({ id: 21, name: '高产源', theme_color: '#8b5cf6' }),
    items: [
      preview({ image: COVER }),
      ...Array.from({ length: 7 }, () => preview()),
    ],
  },
  {
    label: 'list · 头条无图',
    layout: 'list',
    src: source({ id: 22, name: '无图高产源' }),
    items: Array.from({ length: 6 }, () => preview()),
  },
  {
    label: 'cadence · 沉寂 210 天',
    layout: 'cadence',
    src: source({
      id: 31,
      name: '停更的博客',
      theme_color: '#64748b',
      last_success_at: daysAgo(210),
      pulses: [210, 236, 268, 301, 355, 420, 498, 560, 640, 700],
    }),
    items: [preview({ published_at: daysAgo(210) })],
  },
  {
    label: 'cadence · 抓取失败（管理员红色 token）',
    layout: 'cadence',
    src: source({
      id: 32,
      name: '抓不动的源',
      error_count: 7,
      last_success_at: daysAgo(300),
      pulses: [300, 330, 366, 402, 470, 530],
    }),
    items: [preview({ published_at: daysAgo(300) })],
  },
  {
    label: 'numeric · 未读 47',
    layout: 'numeric',
    src: source({ id: 41, name: '积压的源', unread_count: 47 }),
    items: Array.from({ length: 6 }, () => preview()),
  },
  {
    label: 'icon · 友链',
    layout: 'icon',
    src: source({
      id: 51,
      name: '朋友的站',
      source_type: 'link',
      theme_color: '#ec4899',
      category: '友情链接',
    }),
    items: [],
  },
  {
    label: 'icon · 无简介',
    layout: 'icon',
    src: source({
      id: 52,
      name: '只有名字',
      source_type: 'link',
      description: null,
      ai_style_tags: null,
    }),
    items: [],
  },
]

function makeTopic(withCovers: boolean): BrewTopic {
  const items = Array.from({ length: 6 }, (_, i) => ({
    id: 7000 + i,
    title: `跨源主题下的第 ${i + 1} 篇，标题长度用来验证单行截断`,
    image: withCovers && i < 3 ? COVER : null,
    published_at: daysAgo(i + 1),
    topic: 'engineering' as const,
    source_id: 100 + (i % 3),
    source_name: `源 ${(i % 3) + 1}`,
  }))
  return clusterTopics(items, NOW)[0]
}

const ROLES: BrewViewerRole[] = ['guest', 'member', 'admin']

export default function BrewTilePreview() {
  const [role, setRole] = useState<BrewViewerRole>('member')
  const topicWithCovers = useMemo(() => makeTopic(true), [])
  const topicNoCovers = useMemo(() => makeTopic(false), [])

  return (
    <div className="min-h-screen px-6 py-10">
      <div className="mb-6 flex items-center gap-3">
        <h1 className="text-lg font-semibold">Brew 磁贴预览</h1>
        <span className="text-xs text-gray-400">
          16×4 网格下的真实物理尺寸 · NOW 固定为 2025-06-15
        </span>
        <div className="ml-auto flex gap-1">
          {ROLES.map((r) => (
            <button
              key={r}
              type="button"
              onClick={() => setRole(r)}
              className={`rounded-md px-2 py-1 text-xs ${
                role === r
                  ? 'bg-black/10 dark:bg-white/15'
                  : 'text-gray-400 hover:bg-black/5 dark:hover:bg-white/8'
              }`}
            >
              {r}
            </button>
          ))}
        </div>
      </div>

      {CASES.map((c) => (
        <section key={`${c.label}-${c.src.id}`} className="mb-8">
          <h2 className="mb-2 text-xs text-gray-500">{c.label}</h2>
          <div className="flex flex-wrap items-start gap-4">
            {/* Only sizes this source can be assigned; bar/strip are entry-source only. */}
            {allowedTileSizes(c.src).map((size) => (
              <div key={size} className="flex flex-col gap-1">
                <span className="text-[10px] text-gray-400">{size}</span>
                <div style={SIZE_PX[size]}>
                  <BrewSourceTile
                    source={c.src}
                    size={size}
                    role={role}
                    now={NOW}
                    scale={1}
                    fontScale={1}
                    items={c.items}
                    layoutOverride={c.layout}
                  />
                </div>
              </div>
            ))}
          </div>
        </section>
      ))}

      <section className="mb-8">
        <h2 className="mb-2 text-xs text-gray-500">topic · 有封面拼贴</h2>
        <div className="flex flex-wrap items-start gap-4">
          {(['4x2', '4x4'] as BrewTileSize[]).map((size) => (
            <div key={size} className="flex flex-col gap-1">
              <span className="text-[10px] text-gray-400">{size}</span>
              <div style={SIZE_PX[size]}>
                <BrewTopicTile
                  topic={topicWithCovers}
                  size={size}
                  scale={1}
                  fontScale={1}
                />
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="mb-8">
        <h2 className="mb-2 text-xs text-gray-500">
          topic · 一张封面都没有（拼贴返回 null，不补灰块）
        </h2>
        <div className="flex flex-wrap items-start gap-4">
          {(['4x2', '4x4'] as BrewTileSize[]).map((size) => (
            <div key={size} className="flex flex-col gap-1">
              <span className="text-[10px] text-gray-400">{size}</span>
              <div style={SIZE_PX[size]}>
                <BrewTopicTile
                  topic={topicNoCovers}
                  size={size}
                  scale={1}
                  fontScale={1}
                />
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="mb-8">
        <h2 className="mb-2 text-xs text-gray-500">
          featured · 首页综合卡（内部跑 smart）
        </h2>
        <div className="flex flex-wrap items-start gap-4">
          {(['4x2', '4x4'] as BrewTileSize[]).map((size) => (
            <div key={size} className="flex flex-col gap-1">
              <span className="text-[10px] text-gray-400">{size}</span>
              <div style={SIZE_PX[size]}>
                <BrewFeaturedTile
                  size={size}
                  scale={1}
                  fontScale={1}
                  now={NOW}
                  sources={CASES.map((c) => ({
                    ...c.src,
                    recent_items: c.items ?? [],
                  }))}
                  emptyHint="Nothing to show"
                />
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="mb-8">
        <h2 className="mb-2 text-xs text-gray-500">
          featured · 一个源都没有（空态）
        </h2>
        <div style={SIZE_PX['4x2']}>
          <BrewFeaturedTile
            size="4x2"
            scale={1}
            fontScale={1}
            now={NOW}
            sources={[]}
            emptyHint="Nothing to show"
          />
        </div>
      </section>
    </div>
  )
}
