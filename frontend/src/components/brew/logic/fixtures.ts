import type { BrewItem, BrewItemPreview, BrewSource } from '../../../types/brew'

const MS_PER_DAY = 86_400_000

export const NOW = Date.UTC(2025, 5, 15)

export function daysAgo(days: number, now: number = NOW): number {
  return now - days * MS_PER_DAY
}

let previewSeq = 0

export function makePreview(
  over: Partial<BrewItemPreview> = {},
): BrewItemPreview {
  previewSeq += 1
  return {
    id: previewSeq,
    title: `预览条目 ${previewSeq}`,
    summary: null,
    image: null,
    published_at: daysAgo(1),
    is_read: false,
    ...over,
  }
}

export function makePreviews(
  n: number,
  over: Partial<BrewItemPreview> = {},
): BrewItemPreview[] {
  return Array.from({ length: n }, (_, i) =>
    makePreview({ published_at: daysAgo(i + 1), ...over }),
  )
}

export function makeSource(over: Partial<BrewSource> = {}): BrewSource {
  return {
    id: 1,
    user_id: 1,
    name: '示例源',
    url: 'https://example.com/feed.xml',
    feed_type: 'rss',
    source_type: 'rss',
    category: null,
    icon: null,
    description: null,
    site_url: 'https://example.com',
    update_interval: 3600,
    last_fetched_at: daysAgo(1),
    last_success_at: daysAgo(1),
    last_error: null,
    error_count: 0,
    enabled: true,
    item_count: 100,
    unread_count: 0,
    card_size: null,
    theme_color: '#f97316',
    sort_order: null,
    ai_style_tags: null,
    rsshub_route: null,
    admin_only: false,
    created_at: daysAgo(400),
    recent_items: [],
    ...over,
  }
}

let itemSeq = 0

export function makeItem(over: Partial<BrewItem> = {}): BrewItem {
  itemSeq += 1
  return {
    id: 1000 + itemSeq,
    source_id: 1,
    source_name: '示例源',
    source_icon: null,
    guid: `guid-${itemSeq}`,
    title: `文章 ${itemSeq}`,
    link: `https://example.com/${itemSeq}`,
    summary: null,
    content: null,
    image: null,
    audio_url: null,
    author: null,
    published_at: daysAgo(1),
    word_count: 800,
    reading_time: 4,
    is_read: false,
    is_starred: false,
    read_progress: null,
    created_at: daysAgo(1),
    topic: null,
    ...over,
  }
}
