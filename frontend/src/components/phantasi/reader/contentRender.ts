import type { AnnotationItem } from '../../../services/phantasiaiApi'
import type { CommentItem } from '../../../services/phantasiApi'
import type { PhantasiItem } from '../../../types/phantasi'
import type { ReaderCopy, ThemeKey } from './types'
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { API_URL as CONFIG_API_URL } from '../../../config'
import { processEmbeds } from '../../../utils/embedProcessor'
import { escapeHtml } from '../../../utils/inputSanitizer'
import { processRssContentAsync } from '../../../utils/rssContentProcessor'
import { yieldIfSliceExceeded } from '../../../utils/yieldToMain'
import { displayImageUrl, prepareNoteReaderHtml } from '../notes/noteImageUrl'
import { decorateNoteReadSurface } from '../notes/noteReadSurface'
import { replaceNoteHtml } from '../notes/noteWidgetMount'
import {
  commentAnchorStale,
  paintAnchoredAnnotations,
  paintAnchoredComments,
  unwrapTextDecorations,
} from './commentAnchors'
import '../../settings/GitHubProjectBadge.css'
import '../../github/githubRepoCard.css'

const API_URL = CONFIG_API_URL

// 本站媒体（/api、/media/federation）改走当前 API origin；外站图仅 must-proxy 走 `/api/proxy/image`。
export function getImageUrl(imageUrl: string | null): string | null {
  if (!imageUrl) return null
  return displayImageUrl(imageUrl, API_URL)
}

interface BuildBaseContentOptions {
  contentReady: boolean
  item: Pick<
    PhantasiItem,
    'content' | 'summary' | 'link' | 'fromWebSearch' | 'guid'
  >
  t: ReaderCopy
}

function buildBaseContent({
  contentReady,
  item,
  t,
}: BuildBaseContentOptions): string | null {
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
          <p class="web-search-note">${escapeHtml(t.phantasi.webSearchNote)}</p>
        </div>`
    }
    return `<div class="web-search-summary">
        <p class="opacity-60">${escapeHtml(t.phantasi.noSummary)}</p>
      </div>`
  }

  const empty = `<p class="opacity-50">${t.phantasi.noContent}</p>`
  if (item.guid.startsWith('note:')) {
    return prepareNoteReaderHtml(item.content, empty)
  }

  return null
}

async function buildRssContent(
  item: BuildBaseContentOptions['item'],
  empty: string,
  signal: AbortSignal,
): Promise<string> {
  const source = item.content || item.summary || empty
  const cleaned = await processRssContentAsync(
    source,
    {
      lazyLoadImages: true,
      removeTrackingParams: true,
      removeEmptyTags: true,
      baseUrl: item.link || undefined,
    },
    signal,
  )
  signal.throwIfAborted()
  const slice = { ms: performance.now() }
  const withEmbeds = processEmbeds(cleaned)
  await yieldIfSliceExceeded(slice)
  return withEmbeds
}

interface UseContentRenderOptions {
  contentInnerRef: React.RefObject<HTMLDivElement | null>
  contentReady: boolean
  item: Pick<
    PhantasiItem,
    | 'content'
    | 'summary'
    | 'link'
    | 'fromWebSearch'
    | 'content_revision'
    | 'guid'
  >
  t: ReaderCopy
  showAnnotations: boolean
  annotations: AnnotationItem[]
  comments: CommentItem[]
  theme: ThemeKey
  copyCodeLabel: string
  copyTexLabel?: string
}

export function useContentRender({
  contentInnerRef,
  contentReady,
  item,
  t,
  showAnnotations,
  annotations,
  comments,
  theme,
  copyCodeLabel,
  copyTexLabel,
}: UseContentRenderOptions): string {
  // 笔记和搜索摘要仍同步；RSS 清洗按步让出主线程。
  const cheapContent = useMemo(
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
      item.guid,
      t.phantasi.noContent,
      t.phantasi.webSearchNote,
      t.phantasi.noSummary,
    ],
  )
  const rssGeneration = `${item.guid}\0${item.content_revision ?? ''}\0${item.content ?? ''}\0${item.summary ?? ''}\0${item.link ?? ''}`
  const [rss, setRss] = useState({ generation: '', html: '' })

  useEffect(() => {
    if (cheapContent !== null) return
    const empty = `<p class="opacity-50">${t.phantasi.noContent}</p>`
    const generation = rssGeneration
    const controller = new AbortController()
    void (async () => {
      try {
        const html = await buildRssContent(item, empty, controller.signal)
        if (controller.signal.aborted) return
        setRss({ generation, html })
      } catch {
        if (!controller.signal.aborted) setRss({ generation, html: empty })
      }
    })()
    return () => controller.abort()
  }, [
    cheapContent,
    rssGeneration,
    item.content,
    item.summary,
    item.link,
    t.phantasi.noContent,
  ])

  const baseContent =
    cheapContent !== null
      ? cheapContent
      : rss.generation === rssGeneration
        ? rss.html
        : ''

  const prevBaseContentRef = useRef('')

  // 正文变化才挂载 HTML；批注变化仅装饰文本节点。
  // replaceNoteHtml 写完会通知已登记的水合补挂。放进 useEffect 会晚一帧。
  useLayoutEffect(() => {
    const container = contentInnerRef.current
    if (!container) return
    if (!baseContent) {
      if (prevBaseContentRef.current) {
        replaceNoteHtml(container, '')
        prevBaseContentRef.current = ''
      }
      return
    }

    const isBaseChanged = prevBaseContentRef.current !== baseContent
    prevBaseContentRef.current = baseContent

    if (isBaseChanged || container.childElementCount === 0) {
      replaceNoteHtml(container, baseContent)
      decorateNoteReadSurface(container, copyCodeLabel, copyTexLabel)
    } else {
      unwrapTextDecorations(container)
    }

    if (showAnnotations && annotations.length > 0) {
      paintAnchoredAnnotations(container, annotations)
    }
    const liveComments = comments.filter(
      (comment) => !commentAnchorStale(comment, item.content_revision),
    )
    if (liveComments.length > 0) {
      paintAnchoredComments(container, liveComments, theme)
    }
  }, [
    baseContent,
    showAnnotations,
    annotations,
    comments,
    theme,
    item.content_revision,
    copyCodeLabel,
    copyTexLabel,
  ])

  return baseContent
}
