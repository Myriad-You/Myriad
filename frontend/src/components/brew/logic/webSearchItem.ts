import type { BrewItem } from '../../../types/brew'

export interface WebSearchArticleInput {
  id: number
  title: string
  author?: string | null
  sourceName?: string | null
  publishedAt?: string | null
  summary?: string | null
  relevanceReason?: string | null
  link?: string | null
  content?: string | null
}

export function webSearchInList<T extends { id: number; fromWebSearch?: boolean }>(
  items: readonly T[] | undefined,
  articleId: number,
): { item: T; index: number } | null {
  if (!items) return null
  const index = items.findIndex((item) => item.id === articleId)
  const item = items[index]
  if (!item?.fromWebSearch) return null
  return { item, index }
}

export function brewItemFromWebSearch(
  input: WebSearchArticleInput,
  webSearchLabel: string,
): BrewItem {
  return {
    id: input.id,
    source_id: 0,
    source_name: input.sourceName || webSearchLabel,
    source_icon: null,
    guid: `web_search_${input.id}`,
    title: input.title,
    link: input.link || '',
    summary: input.summary || input.relevanceReason || null,
    content: input.content || null,
    image: null,
    audio_url: null,
    author: input.author || null,
    published_at: input.publishedAt
      ? new Date(input.publishedAt).getTime()
      : null,
    word_count: null,
    reading_time: null,
    is_read: false,
    is_starred: false,
    read_progress: null,
    created_at: Date.now(),
    fromWebSearch: true,
  }
}
