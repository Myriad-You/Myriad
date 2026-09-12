import type { ReactNode } from 'react'

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useSyncExternalStore,
} from 'react'
import { authSubject } from '../utils/authSubject'
import { currentPagePublisher, getCurrentPageContent, subscribeCurrentPageContent } from './currentPage'

const subscribeSubject = (listener: () => void) => authSubject.subscribe(listener)
const subjectSnapshot = () => authSubject.signal
const serverPageSnapshot = () => null

export type PageContentType =
  'brew_article' | 'tapp_data' | 'platform_data' | 'custom'

export interface PageContent {
  type: PageContentType
  title?: string
  summary?: string
  content?: string
  plainText?: string
  sourceUrl?: string
  author?: string
  publishedAt?: string
  metadata?: Record<string, unknown>
}

interface PageContentContextValue {
  pageContent: PageContent | null
  setPageContent: (content: PageContent | null) => void
  clearPageContent: () => void
  hasContent: boolean
  getContentForAgent: () => Record<string, unknown> | null
}

const PageContentContext = createContext<PageContentContextValue | null>(null)

function extractPlainText(html: string): string {
  const temp = document.createElement('div')
  temp.innerHTML = html

  const scripts = temp.querySelectorAll('script, style')
  scripts.forEach((el) => el.remove())

  const text = temp.textContent || ''

  return text.replaceAll(/\s+/g, ' ').trim()
}

function truncateText(text: string, maxLength: number): string {
  if (text.length <= maxLength) return text
  return `${text.slice(0, maxLength)}...`
}

export function PageContentProvider({ children }: { children: ReactNode }) {
  const subject = useSyncExternalStore(subscribeSubject, subjectSnapshot, subjectSnapshot)
  const pageContent = useSyncExternalStore(subscribeCurrentPageContent, getCurrentPageContent, serverPageSnapshot)
  const publish = useMemo(() => currentPagePublisher(subject), [subject])

  const setPageContent = useCallback((content: PageContent | null) => {
    if (subject.aborted) return
    content = content ? { ...content } : null
    if (content) {
      if (content.content && !content.plainText) {
        content.plainText = extractPlainText(content.content)
      }
      if (!content.summary && content.plainText) {
        content.summary = truncateText(content.plainText, 200)
      }
    }
    publish(content)
  }, [publish, subject])

  const clearPageContent = useCallback(() => {
    publish(null)
  }, [publish])

  const hasContent = pageContent !== null

  const getContentForAgent = useCallback((): Record<string, unknown> | null => {
    if (subject.aborted || !pageContent) return null

    return {
      type: pageContent.type,
      title: pageContent.title,
      summary: pageContent.summary,
      // Cap Agent payload length.
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
  }, [pageContent, subject])

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

export function usePageContent() {
  const context = useContext(PageContentContext)
  if (!context) {
    throw new Error('usePageContent must be used within a PageContentProvider')
  }
  return context
}

/** Returns null outside the provider. */
export function usePageContentOptional() {
  return useContext(PageContentContext)
}
