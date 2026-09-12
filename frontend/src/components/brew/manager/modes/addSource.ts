/** 不碰 DOM。 */

import type { AddSourceInput, FeedType, SourceType } from '../../../../types/brew'

export type AddSourceKind = Extract<
  SourceType,
  'rss' | 'brewlia' | 'link' | 'rsshub'
>

export type AddFeedKind = Extract<FeedType, 'rss' | 'atom' | 'notion' | 'rsshub'>

export type AddFieldKind = 'notion' | 'rsshub' | 'link' | 'rss'

export function addFieldKind(
  sourceType: AddSourceKind,
  feedType: AddFeedKind,
): AddFieldKind {
  if (feedType === 'notion') return 'notion'
  if (sourceType === 'rsshub') return 'rsshub'
  if (sourceType === 'link') return 'link'
  return 'rss'
}

export function canAutoDiscover(
  sourceType: AddSourceKind,
  feedType: AddFeedKind,
): boolean {
  return addFieldKind(sourceType, feedType) === 'rss'
}

export function resolveAddSourceType(
  sourceType: AddSourceKind,
  enableBrewliaForRsshub: boolean,
): AddSourceKind {
  return sourceType === 'rsshub' && enableBrewliaForRsshub
    ? 'brewlia'
    : sourceType
}

export function canSubmitAdd(input: {
  sourceType: AddSourceKind
  url: string
  name: string
  rsshubFullUrl: string
}): boolean {
  if (input.sourceType === 'rsshub') return !!input.rsshubFullUrl
  if (!input.url.trim()) return false
  if (input.sourceType === 'link' && !input.name.trim()) return false
  return true
}

export function faviconForUrl(url: string): string | null {
  try {
    return `https://www.google.com/s2/favicons?sz=64&domain=${new URL(url).hostname}`
  } catch {
    return null
  }
}

export interface DiscoveredFeed {
  url: string
  autocompleted: boolean
  title: string
  feed_type: string
}

export function pickAddKind(kind: AddFieldKind): {
  sourceType: AddSourceKind
  feedType: AddFeedKind
  clearUrl: boolean
} {
  if (kind === 'link') return { sourceType: 'link', feedType: 'rss', clearUrl: false }
  if (kind === 'rsshub') {
    return { sourceType: 'rsshub', feedType: 'rsshub', clearUrl: true }
  }
  if (kind === 'notion') {
    return { sourceType: 'rss', feedType: 'notion', clearUrl: false }
  }
  return { sourceType: 'rss', feedType: 'rss', clearUrl: false }
}

export function addHintKey(
  kind: AddFieldKind,
): 'notionDesc' | 'rsshubDesc' | 'linkDesc' | 'rssDesc' {
  if (kind === 'notion') return 'notionDesc'
  if (kind === 'rsshub') return 'rsshubDesc'
  if (kind === 'link') return 'linkDesc'
  return 'rssDesc'
}

export function addUrlLabelKey(
  kind: AddFieldKind,
): 'notionUrlLabel' | 'linkUrlLabel' | 'subscriptionUrlLabel' {
  if (kind === 'notion') return 'notionUrlLabel'
  if (kind === 'link') return 'linkUrlLabel'
  return 'subscriptionUrlLabel'
}

export function addUrlPlaceholder(kind: AddFieldKind): string {
  if (kind === 'notion') return 'notion://database/xxx'
  if (kind === 'link') return 'https://example.com'
  return 'https://example.com/feed.xml'
}

export function addSubmitLabelKey(
  sourceType: AddSourceKind,
): 'addLink' | 'addBrewlia' | 'addRsshub' | 'addSubscription' {
  if (sourceType === 'link') return 'addLink'
  if (sourceType === 'brewlia') return 'addBrewlia'
  if (sourceType === 'rsshub') return 'addRsshub'
  return 'addSubscription'
}

export function isOpmlFilename(name: string): boolean {
  return name.endsWith('.opml') || name.endsWith('.xml')
}

export function toAddSourceInput(data: {
  url: string
  name: string
  category: string
  customIcon: string | null
  sourceType: SourceType
  feedType: FeedType
  notionToken?: string
}): AddSourceInput {
  return {
    url: data.url,
    name: data.name.trim() || undefined,
    category: data.category.trim() || undefined,
    icon: data.customIcon || undefined,
    sourceType: data.sourceType,
    feedType: data.feedType,
    notionToken: data.notionToken?.trim() || undefined,
  }
}
