/**
 * 页面内容上下文
 *
 * 用于在应用中共享当前页面的内容数据
 * 主要场景：
 * - Brew 阅读器中的文章内容
 * - Tapp 页面的数据
 * - 其他需要 Agent 访问的页面内容
 */

import type { ReactNode } from 'react'

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from 'react'
import { setCurrentPageContent } from './currentPage'

/** 页面内容类型 */
export type PageContentType =
  'brew_article' | 'tapp_data' | 'platform_data' | 'custom'

/** 页面内容数据 */
export interface PageContent {
  /** 内容类型 */
  type: PageContentType
  title?: string
  /** 内容摘要（用于快速预览） */
  summary?: string
  /** 完整内容（HTML 或纯文本） */
  content?: string
  /** 纯文本内容（从 HTML 提取） */
  plainText?: string
  /** 来源 URL */
  sourceUrl?: string
  /** 作者 */
  author?: string
  /** 发布时间 */
  publishedAt?: string
  /** 额外元数据 */
  metadata?: Record<string, unknown>
}

/** 上下文值类型 */
interface PageContentContextValue {
  /** 当前页面内容 */
  pageContent: PageContent | null
  /** 设置页面内容 */
  setPageContent: (content: PageContent | null) => void
  /** 清除页面内容 */
  clearPageContent: () => void
  /** 是否有内容 */
  hasContent: boolean
  /** 获取用于 Agent 的内容摘要 */
  getContentForAgent: () => Record<string, unknown> | null
}

const PageContentContext = createContext<PageContentContextValue | null>(null)

/** 从 HTML 提取纯文本 */
function extractPlainText(html: string): string {
  // 创建临时 DOM 元素
  const temp = document.createElement('div')
  temp.innerHTML = html

  // 移除脚本和样式
  const scripts = temp.querySelectorAll('script, style')
  scripts.forEach((el) => el.remove())

  // 获取文本内容
  const text = temp.textContent || ''

  // 清理多余空白
  return text.replace(/\s+/g, ' ').trim()
}

/** 截断文本 */
function truncateText(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text
  return `${text.slice(0, maxLength)}...`
}

export function PageContentProvider({ children }: { children: ReactNode }) {
  const [pageContent, setPageContentState] = useState<PageContent | null>(null)

  const setPageContent = useCallback((content: PageContent | null) => {
    if (content) {
      // 如果有 HTML 内容但没有纯文本，自动提取
      if (content.content && !content.plainText) {
        content.plainText = extractPlainText(content.content)
      }
      // 如果没有摘要，从纯文本生成
      if (!content.summary && content.plainText) {
        content.summary = truncateText(content.plainText, 200)
      }
    }
    setPageContentState(content)
    setCurrentPageContent(content)
  }, [])

  const clearPageContent = useCallback(() => {
    setPageContentState(null)
    setCurrentPageContent(null)
  }, [])

  const hasContent = pageContent !== null

  const getContentForAgent = useCallback((): Record<string, unknown> | null => {
    if (!pageContent) return null

    return {
      type: pageContent.type,
      title: pageContent.title,
      summary: pageContent.summary,
      // 限制传递给 Agent 的内容长度，避免 token 过长
      content: pageContent.plainText
        ? truncateText(pageContent.plainText, 10000)
        : pageContent.content
          ? truncateText(extractPlainText(pageContent.content), 10000)
          : null,
      sourceUrl: pageContent.sourceUrl,
      author: pageContent.author,
      publishedAt: pageContent.publishedAt,
      metadata: pageContent.metadata,
    }
  }, [pageContent])

  const value = useMemo(
    () => ({
      pageContent,
      setPageContent,
      clearPageContent,
      hasContent,
      getContentForAgent,
    }),
    [
      pageContent,
      setPageContent,
      clearPageContent,
      hasContent,
      getContentForAgent,
    ],
  )

  return (
    <PageContentContext.Provider value={value}>
      {children}
    </PageContentContext.Provider>
  )
}

/** 使用页面内容上下文 */
export function usePageContent() {
  const context = useContext(PageContentContext)
  if (!context) {
    throw new Error('usePageContent must be used within a PageContentProvider')
  }
  return context
}

/** 可选的页面内容 hook（不报错，返回空值） */
export function usePageContentOptional() {
  return useContext(PageContentContext)
}
