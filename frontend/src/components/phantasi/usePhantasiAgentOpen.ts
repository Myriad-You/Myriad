/** skin 不进口。 */

import type { MutableRefObject } from 'react'
import type { ReadingList } from '../../contexts/ReadingListContext'

import type { PhantasiItem } from '../../types/phantasi'
import type { ArticleLoader, OpenArticleOptions } from './useArticleOpen'
import { useEffect, useRef } from 'react'
import * as phantasiApi from '../../services/phantasiApi'
import { emitAppEvent } from '../../utils/appEvents'
import { phantasiSubject } from '../../utils/phantasiSubject'
import {
  findAgentArticle,
  PENDING_OPEN_KEY,
  PENDING_READING_KEY,
  takeFreshPending,
  wantsLatestOnly,
} from './logic/agentOpen'
import { readingQueue } from './logic/readingQueue'
import { phantasiItemFromWebSearch } from './logic/webSearchItem'

interface AgentOpenDetail {
  subjectKey?: string
  subjectGeneration?: number
  articleId?: string
  articleLink?: string
  openLatest?: boolean
  timestamp?: number
  webSearchArticle?: {
    id: number
    title: string
    author?: string
    sourceName?: string
    publishedAt?: string
    summary?: string
    relevanceReason?: string
    link?: string
    content?: string
    fromWebSearch?: boolean
    isWebSearchArticle?: boolean
  }
  /** A ReadingList as AgentGlobalActions serialized it into sessionStorage. */
  readingList?: Omit<ReadingList, 'createdAt'> & { createdAt: string }
}

interface AgentOpenIo {
  itemsRef: MutableRefObject<PhantasiItem[]>
  openArticle: (
    target: PhantasiItem | ArticleLoader,
    options?: OpenArticleOptions,
  ) => Promise<PhantasiItem | undefined>
  webSearchLabel: string
}

export function usePhantasiAgentOpen(io: AgentOpenIo) {
  const ioRef = useRef(io)
  ioRef.current = io

  useEffect(() => {
    const timers: number[] = []

    const handleAgentOpenArticle = async (e: Event) => {
      const { itemsRef, openArticle, webSearchLabel } = ioRef.current
      const { articleId, articleLink, openLatest, webSearchArticle } = (
        e as CustomEvent<AgentOpenDetail>
      ).detail
      const list = (e as CustomEvent<AgentOpenDetail>).detail.readingList
      const queue = Array.isArray(list?.items)
        ? readingQueue('agent', list.items, list.name)
        : null
      if (webSearchArticle?.isWebSearchArticle) {
        await openArticle(
          phantasiItemFromWebSearch(webSearchArticle, webSearchLabel),
          { queue },
        )
        return
      }
      await openArticle(async (signal) => {
        const check = () => signal.throwIfAborted()
        if (articleId || articleLink) {
          const found = await findAgentArticle<
            Pick<PhantasiItem, 'id' | 'title' | 'link' | 'guid'>
          >(
            { articleId, articleLink },
            itemsRef.current,
            {
              getById: async (id) => {
                check()
                return phantasiApi.getItem(id, undefined, { signal })
              },
              getPage: async (page, perPage) => {
                check()
                return phantasiApi.getItemPreviews(
                  {
                    page,
                    per_page: perPage,
                    filter: 'all',
                  },
                  undefined,
                  { signal },
                )
              },
            },
          )
          if (!found) return null
          if (Object.hasOwn(found, 'content')) return found as PhantasiItem
          check()
          return phantasiApi.getItem(found.id, undefined, { signal })
        }
        if (wantsLatestOnly({ articleId, articleLink, openLatest })) {
          const data = await phantasiApi.getItemPreviews(
            { per_page: 1, filter: 'all' },
            undefined,
            { signal },
          )
          const latest = data.items[0]
          if (!latest) return null
          check()
          return phantasiApi.getItem(latest.id, undefined, { signal })
        }
        return null
      }, { queue })
    }

    window.addEventListener('agent:open-phantasi-article', handleAgentOpenArticle)

    const takePending = (key: string) => {
      const taken = takeFreshPending<AgentOpenDetail>(
        sessionStorage.getItem(key),
        Date.now(),
      )
      if (taken.ok) {
        sessionStorage.removeItem(key)
        const subject = phantasiSubject.capture()
        if (
          taken.value.subjectKey !== subject.key ||
          taken.value.subjectGeneration !== subject.generation
        ) { return null
}
        return taken.value
      }
      if (taken.reason === 'missing') return null
      sessionStorage.removeItem(key)
      if (taken.reason === 'expired') {
        /* drop stale handoff */
      } else {
        console.error(
          key === PENDING_READING_KEY
            ? '[Phantasi] Failed to parse pending reading list:'
            : '[Phantasi] Failed to parse pending action:',
          taken.reason,
        )
      }
      return null
    }

    const pendingReading = takePending(PENDING_READING_KEY)
    if (pendingReading) {
      if (pendingReading.readingList) {
        emitAppEvent('agent:set-reading-list', {
          ...pendingReading.readingList,
          createdAt: new Date(pendingReading.readingList.createdAt),
        })
      }
      if (pendingReading.articleId) {
        timers.push(
          window.setTimeout(() => {
            void handleAgentOpenArticle(
              new CustomEvent('agent:open-phantasi-article', {
                detail: {
                  articleId: pendingReading.articleId,
                  openLatest: false,
                  webSearchArticle: pendingReading.webSearchArticle,
                  readingList: pendingReading.readingList,
                },
              }),
            )
          }, 200),
        )
      }
    }

    const pendingOpen = takePending(PENDING_OPEN_KEY)
    if (pendingOpen) {
      timers.push(
        window.setTimeout(() => {
          void handleAgentOpenArticle(
            new CustomEvent('agent:open-phantasi-article', {
              detail: pendingOpen,
            }),
          )
        }, 100),
      )
    }

    return () => {
      window.removeEventListener(
        'agent:open-phantasi-article',
        handleAgentOpenArticle,
      )
      for (const id of timers) window.clearTimeout(id)
    }
  }, [])
}
