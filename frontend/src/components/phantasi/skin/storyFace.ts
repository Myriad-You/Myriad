import type { FeedStory } from '../logic/feedStories'
import type { TopicNameKey } from '../logic/topics'
import type { TimeTranslations } from '../types'

import { phantasiSubject } from '../../../utils/phantasiSubject'
import { getIconUrl, getImageUrl, getPlainText } from '../constants'
import { topicDisplayName, topicHue } from '../logic/topics'
import { noteStoryTopic } from '../notes/noteCategory'
import { isNoteStorySource, siteStoryAttribution, storySourceFace } from '../notes/noteSiteSource'
import { phantasiRelativeTime } from './time'

export interface StoryCardFace {
  id: number
  title: string
  summary: string
  cover: string | null
  source: string
  sourceIcon: string | null
  when: string
  topic: string | null
  hue: string | null
  author: string | null | undefined
  unread: boolean
  starred: boolean
}

interface FaceLabels extends Partial<Record<TopicNameKey, string>> {
  unread: string
  starred: string
  unstar: string
}

interface FaceHit {
  times: TimeTranslations
  locale: string
  labels: FaceLabels
  unread: boolean
  starred: boolean
  siteName: string
  siteIcon: string | null
  face: StoryCardFace
}

const faces = new WeakMap<FeedStory, FaceHit>()
const inners = new WeakMap<StoryCardFace, Map<number, string>>()

function escapeStoryText(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;')
}

/** 订阅轨提交时只写一段 HTML，不再为每张卡建一整棵 React 子树。 */
export function storyCardInnerHtml(
  face: StoryCardFace,
  unreadLabel: string | undefined,
  starLabel: string | undefined,
  unstarLabel: string | undefined,
  deferCover: boolean,
  showStar: boolean,
): string {
  const key = (deferCover ? 1 : 0) | (showStar ? 2 : 0)
  let cache = inners.get(face)
  if (!cache) {
    cache = new Map()
    inners.set(face, cache)
  }
  const hit = cache.get(key)
  if (hit) return hit
  const topic = face.topic
    ? `<span class="phantasi-story__topic">${escapeStoryText(face.topic)}</span>`
    : ''
  let source = ''
  if (face.source || face.sourceIcon) {
    const dataSrc =
      deferCover && face.sourceIcon
        ? ` data-src="${escapeStoryText(face.sourceIcon)}"`
        : ''
    let mark = ''
    if (face.sourceIcon && !deferCover) {
      const icon = escapeStoryText(face.sourceIcon)
      mark =
        `<img src="${icon}" data-src="${icon}" alt="" loading="lazy" decoding="async">`
    } else if (!face.sourceIcon && face.source) {
      mark =
        `<span class="phantasi-story__source-mark" aria-hidden>${escapeStoryText(face.source.slice(0, 1))}</span>`
    }
    const name = face.source
      ? `<span>${escapeStoryText(face.source)}</span>`
      : ''
    source = `<span class="phantasi-story__source"${dataSrc}>${mark}${name}</span>`
  }
  const when = face.when
    ? `<span class="phantasi-story__meta">${escapeStoryText(face.when)}</span>`
    : ''
  const unread =
    face.unread && unreadLabel
      ? `<span class="phantasi-story__unread">${escapeStoryText(unreadLabel)}</span>`
      : ''
  const author = face.author
    ? `<span class="phantasi-story__byline">${escapeStoryText(face.author)}</span>`
    : ''
  let thumb = ''
  if (face.cover) {
    const cover = escapeStoryText(face.cover)
    const held = deferCover ? ` data-src="${cover}"` : ''
    const img = deferCover
      ? ''
      : `<img src="${cover}" data-src="${cover}" alt="" loading="lazy" decoding="async">`
    thumb =
      `<span class="phantasi-story__thumb" aria-hidden${held}>${img}</span>`
  }
  const summary = face.summary
    ? `<span class="phantasi-story__summary">${escapeStoryText(face.summary)}</span>`
    : ''
  let star = ''
  if (showStar) {
    const label = escapeStoryText(
      (face.starred ? unstarLabel : starLabel) ?? '',
    )
    star =
      `<span class="phantasi-story__star${face.starred ? ' is-on' : ''}" title="${label}" aria-label="${label}" aria-pressed="${face.starred ? 'true' : 'false'}" role="button"></span>`
  }
  const next = [
    `<span class="phantasi-story__kicker">${topic}${source}${when}${unread}</span>`,
    `<span class="phantasi-story__title">${escapeStoryText(face.title)}</span>`,
    author,
    thumb,
    summary,
    star,
  ].join('')
  cache.set(key, next)
  return next
}

const templates = new WeakMap<StoryCardFace, Map<number, HTMLTemplateElement>>()

/** 空闲时把 HTML 解析进 template，滚动补绘只 clone。 */
export function primeStoryCardTemplate(
  face: StoryCardFace,
  html: string,
  key: number,
): void {
  if (typeof document === 'undefined') return
  if (typeof document.createElement !== 'function') return
  let cache = templates.get(face)
  if (!cache) {
    cache = new Map()
    templates.set(face, cache)
  }
  if (cache.has(key)) return
  const tpl = document.createElement('template')
  if (!('content' in tpl)) return
  tpl.innerHTML = html
  cache.set(key, tpl)
}

export function cloneStoryCardInner(
  face: StoryCardFace,
  key: number,
): Node | null {
  const tpl = templates.get(face)?.get(key)
  if (!tpl?.content || typeof tpl.content.cloneNode !== 'function') return null
  return tpl.content.cloneNode(true)
}

export function storyCardFace(
  item: FeedStory,
  times: TimeTranslations,
  locale: string,
  labels: FaceLabels,
): StoryCardFace {
  // 游客不记录阅读状态（打开不写已读，后端给的 is_read 恒为 false），「未读」对他们只是噪音。
  const unread =
    !item.is_read && phantasiSubject.getSnapshot().key.startsWith('user:')
  const starred = !!item.is_starred
  const site = siteStoryAttribution()
  const hit = faces.get(item)
  if (
    hit
    && hit.times === times
    && hit.locale === locale
    && hit.labels === labels
    && hit.unread === unread
    && hit.starred === starred
    && hit.siteName === site.name
    && hit.siteIcon === site.icon
  ) {
    return hit.face
  }
  const source = storySourceFace(item, site)
  const noteTopic = isNoteStorySource(item)
    ? noteStoryTopic(item.topic, labels)
    : null
  const face: StoryCardFace = {
    id: item.id,
    title: item.title,
    summary: item.summary ? getPlainText(item.summary, 480) : '',
    cover: getImageUrl(item.image),
    source: source.name,
    sourceIcon: getIconUrl(source.icon),
    when: phantasiRelativeTime(item.published_at, times, locale),
    topic: noteTopic
      ? noteTopic.topic
      : item.topic
        ? topicDisplayName({ key: item.topic }, labels)
        : null,
    hue: noteTopic
      ? noteTopic.hue
      : item.topic
        ? topicHue(item.topic)
        : null,
    author: item.author,
    unread,
    starred,
  }
  faces.set(item, {
    times,
    locale,
    labels,
    unread,
    starred,
    siteName: site.name,
    siteIcon: site.icon,
    face,
  })
  return face
}

/** 空闲时先算下一窗的面，扩窗提交时不再剥摘要。 */
export function warmStoryFaces(
  items: readonly FeedStory[],
  times: TimeTranslations,
  locale: string,
  labels: FaceLabels,
  deferOnly = false,
): void {
  for (const item of items) {
    const face = storyCardFace(item, times, locale, labels)
    const defer = storyCardInnerHtml(
      face,
      labels.unread,
      labels.starred,
      labels.unstar,
      true,
      true,
    )
    primeStoryCardTemplate(face, defer, 3)
    if (deferOnly) continue
    const full = storyCardInnerHtml(
      face,
      labels.unread,
      labels.starred,
      labels.unstar,
      false,
      true,
    )
    primeStoryCardTemplate(face, full, 2)
  }
}

const warmedCoverUrls = new Set<string>()
const WARM_COVER_URL_CAP = 80
const WARM_COVER_ITEM_MAX = 8

function warmCoverUrl(url: string | null): void {
  if (!url || warmedCoverUrls.has(url) || typeof Image === 'undefined') return
  if (warmedCoverUrls.size >= WARM_COVER_URL_CAP) {
    const first = warmedCoverUrls.values().next().value
    if (first != null) warmedCoverUrls.delete(first)
  }
  warmedCoverUrls.add(url)
  const img = new Image()
  img.decoding = 'async'
  img.src = url
}

/** 空闲时先解码下一窗封面，预显进画时不再现拉。 */
export function warmStoryCovers(items: readonly FeedStory[]): void {
  let n = 0
  for (const item of items) {
    if (n >= WARM_COVER_ITEM_MAX) return
    warmCoverUrl(getImageUrl(item.image))
    warmCoverUrl(getIconUrl(item.source_icon ?? null))
    n += 1
  }
}
