/**
 * 阅读器基础 HTML 构建，以及注释/评论覆盖时保留 iframe 的渲染
 */

import type { CommentItem } from '../../../services/brewApi'
import type { AnnotationItem } from '../../../services/brewliaApi'
import type { BrewItem } from '../../../types/brew'
import type { ThemeKey } from './types'
import { useEffect, useMemo, useRef } from 'react'
import { API_URL as CONFIG_API_URL } from '../../../config'
import * as brewliaApi from '../../../services/brewliaApi'
import { processEmbeds } from '../../../utils/embedProcessor'
import { escapeHtml } from '../../../utils/inputSanitizer'
import { proxyImageUrl } from '../../../utils/proxyImageUrl'
import { processRssContent } from '../../../utils/rssContentProcessor'
import { restoreEmbedElements, saveEmbedElements } from './embedRestore'
import '../../settings/GitHubProjectBadge.css'
import '../../github/githubRepoCard.css'

const API_URL = CONFIG_API_URL

// Dual-path: hotlink CDNs via proxy; otherwise original URL for display
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
  isDark: boolean
  t: Record<string, any>
}

export function buildBaseContent({
  contentReady,
  item,
  isDark,
  t,
}: BuildBaseContentOptions): string {
  if (!contentReady) return ''

  // 网络搜索文章：直接显示 AI 生成的摘要（不再支持加载原文）
  // 必须 HTML 转义，禁止把模型/搜索文本当 HTML 注入
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

  // 0. 首先处理 RSS 内容格式（清理危险标签、适配各类 HTML 标签样式）
  content = processRssContent(content, {
    isDark,
    lazyLoadImages: true,
    removeTrackingParams: true,
    removeEmptyTags: true,
    baseUrl: item.link || undefined,
  })

  // 1. 处理嵌入内容（iframe、特定链接转卡片）
  content = processEmbeds(content, isDark)
  return content
}

export interface UseContentRenderOptions {
  contentInnerRef: React.RefObject<HTMLDivElement | null>
  contentReady: boolean
  item: Pick<BrewItem, 'content' | 'summary' | 'link' | 'fromWebSearch'>
  isDark: boolean
  t: Record<string, any>
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
  isDark,
  t,
  showAnnotations,
  annotations,
  comments,
  highlightComments,
  theme,
}: UseContentRenderOptions): string {
  // 基础内容（不含注释/评论高亮）—— 仅在文章内容或暗色模式变化时重新计算
  // 与 iframe 嵌入等重型内容绑定，避免频繁重建导致闪烁
  const baseContent = useMemo(
    () =>
      buildBaseContent({
        contentReady,
        item,
        isDark,
        t,
      }),
    [
      contentReady,
      item.content,
      item.summary,
      item.link,
      item.fromWebSearch,
      isDark,
      t.brew.noContent,
      t.brew.webSearchNote,
      t.brew.noSummary,
    ],
  )

  const prevBaseContentRef = useRef('')

  // 统一内容渲染：基础内容变化时全量更新，仅注释/评论变化时保留已加载的 iframe
  useEffect(() => {
    const container = contentInnerRef.current
    if (!container || !baseContent) return

    const isBaseChanged = prevBaseContentRef.current !== baseContent
    prevBaseContentRef.current = baseContent

    // 构建包含注释和评论高亮的最终 HTML
    let displayHtml = baseContent
    if (showAnnotations && annotations.length > 0) {
      displayHtml = brewliaApi.highlightAnnotations(displayHtml, annotations)
    }
    if (comments.length > 0) {
      displayHtml = highlightComments(displayHtml, comments, theme)
    }

    if (!isBaseChanged && container.childElementCount > 0) {
      // 仅覆盖层变化：保留已加载的 iframe 和嵌入卡片避免闪烁和重复 API 调用
      const saved = saveEmbedElements(container)
      container.innerHTML = displayHtml
      restoreEmbedElements(container, saved)
    } else {
      // 基础内容变化：直接全量替换
      container.innerHTML = displayHtml
    }
  }, [
    baseContent,
    showAnnotations,
    annotations,
    comments,
    highlightComments,
    theme,
  ])

  return baseContent
}
