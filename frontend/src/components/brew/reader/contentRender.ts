import type { CommentItem } from '../../../services/brewApi'
import type { AnnotationItem } from '../../../services/brewliaApi'
import type { BrewItem } from '../../../types/brew'
import type { ReaderCopy, ThemeKey } from './types'
import { useEffect, useMemo, useRef } from 'react'
import { API_URL as CONFIG_API_URL } from '../../../config'
import { processEmbeds } from '../../../utils/embedProcessor'
import { escapeHtml } from '../../../utils/inputSanitizer'
import { proxyImageUrl } from '../../../utils/proxyImageUrl'
import { processRssContent } from '../../../utils/rssContentProcessor'
import {
  commentAnchorStale,
  highlightAnchoredAnnotations,
} from './commentAnchors'
import { applyTextDecorations } from './textDecorations'
import '../../settings/GitHubProjectBadge.css'
import '../../github/githubRepoCard.css'

const API_URL = CONFIG_API_URL

// 仅 must-proxy 走 `/api/proxy/image`。
export function getImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl) return null
  if (imageUrl.startsWith('/api/') || imageUrl.startsWith(`${API_URL}/api/`)) {
    return imageUrl.startsWith('/api/') ? `${API_URL}${imageUrl}` : imageUrl
  }
  return proxyImageUrl(imageUrl) ?? imageUrl
}

export interface BuildBaseContentOptions {
  contentReady: boolean
  item: Pick<BrewItem, 'content' | 'summary' | 'link' | 'fromWebSearch'>
  t: ReaderCopy
}

export function buildBaseContent({
  contentReady,
  item,
  t,
}: BuildBaseContentOptions): string {
  if (!contentReady) return ''

  // 搜索摘要必须 HTML 转义，禁止当 HTML 注入。
  if (item.fromWebSearch && !item.content) {
    const hasSummary = item.summary && item.summary.trim().length > 20
    if (hasSummary) {
      const paragraphs = item
        .summary!.split(/\n\n|\n/)
        .filter((p) => p.trim())
      const summaryHtml = paragraphs
        .map((p) => `<p>${escapeHtml(p.trim())}</p>`)
        .join('\n')
      return `<div class="web-search-summary">
          ${summaryHtml}
          <p class="web-search-note">${escapeHtml(t.brew.webSearchNote)}</p>
        </div>`
    }
    return `<div class="web-search-summary">
        <p class="opacity-60">${escapeHtml(t.brew.noSummary)}</p>
      </div>`
  }

  let content =
    item.content ||
    item.summary ||
    `<p class="opacity-50">${t.brew.noContent}</p>`

  content = processRssContent(content, {
    lazyLoadImages: true,
    removeTrackingParams: true,
    removeEmptyTags: true,
    baseUrl: item.link || undefined,
  })

  content = processEmbeds(content)
  return content
}

export interface UseContentRenderOptions {
  contentInnerRef: React.RefObject<HTMLDivElement | null>
  contentReady: boolean
  item: Pick<
    BrewItem,
    'content' | 'summary' | 'link' | 'fromWebSearch' | 'content_revision'
  >
  t: ReaderCopy
  showAnnotations: boolean
  annotations: AnnotationItem[]
  comments: CommentItem[]
  highlightComments: (
    html: string,
    commentList: CommentItem[],
    theme: ThemeKey,
  ) => string
  theme: ThemeKey
}

export function useContentRender({
  contentInnerRef,
  contentReady,
  item,
  t,
  showAnnotations,
  annotations,
  comments,
  highlightComments,
  theme,
}: UseContentRenderOptions): string {
  // 正文版本变化才重建 base HTML；主题由 CSS 变量驱动，避免 iframe 被摘下。
  const baseContent = useMemo(
    () =>
      buildBaseContent({
        contentReady,
        item,
        t,
      }),
    [
      contentReady,
      item.content,
      item.summary,
      item.link,
      item.fromWebSearch,
      t.brew.noContent,
      t.brew.webSearchNote,
      t.brew.noSummary,
    ],
  )

  const prevBaseContentRef = useRef('')

  // 正文变化才挂载 HTML；批注变化仅装饰文本节点。
  useEffect(() => {
    const container = contentInnerRef.current
    if (!container || !baseContent) return

    const isBaseChanged = prevBaseContentRef.current !== baseContent
    prevBaseContentRef.current = baseContent

    let displayHtml = baseContent
    if (showAnnotations && annotations.length > 0) {
      displayHtml = highlightAnchoredAnnotations(displayHtml, annotations)
    }
    const liveComments = comments.filter(
      (comment) => !commentAnchorStale(comment, item.content_revision),
    )
    if (liveComments.length > 0) {
      displayHtml = highlightComments(displayHtml, liveComments, theme)
    }

    if (!isBaseChanged && container.childElementCount > 0) {
      // 仅替换文本片段，不摘下 iframe 或其祖先。
      applyTextDecorations(container, displayHtml)
    } else {
      container.innerHTML = displayHtml
    }
  }, [
    baseContent,
    showAnnotations,
    annotations,
    comments,
    highlightComments,
    theme,
    item.content_revision,
  ])

  return baseContent
}
