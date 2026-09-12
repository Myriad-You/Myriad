/** skin 不进口。 */

import type { Dispatch, MutableRefObject, SetStateAction } from 'react'
import type { BrewItem } from '../../types/brew'

import type { ArticleLoader, OpenArticleOptions } from './useArticleOpen'
import { useEffect, useRef } from 'react'
import * as brewApi from '../../services/brewApi'
import { brewSubject } from '../../utils/brewSubject'
import {
  findAgentArticle,
  PENDING_OPEN_KEY,
  PENDING_READING_KEY,
  takeFreshPending,
  wantsLatestOnly,
} from './logic/agentOpen'
import { readingQueue } from './logic/readingQueue'
import { brewItemFromWebSearch } from './logic/webSearchItem'

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
  readingList?: {
    createdAt: string
    name?: string
    items?: Array<{ id: number; title: string }>
    [key: string]: unknown
  }
}

interface AgentOpenIo {
  itemsRef: MutableRefObject<BrewItem[]>
  openArticle: (
    target: BrewItem | ArticleLoader,
    options?: OpenArticleOptions,
  ) => Promise<BrewItem | undefined>
  setItems: Dispatch<SetStateAction<BrewItem[]>>
  setTotal: Dispatch<SetStateAction<number>>
  setError: (message: string) => void
  webSearchLabel: string
  loadFailed: string
}

export function useBrewAgentOpen(io: AgentOpenIo) {
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
          brewItemFromWebSearch(webSearchArticle, webSearchLabel),
          { queue },
        )
        return
      }
      await openArticle(async (signal) => {
        const check = () => signal.throwIfAborted()
        if (articleId || articleLink) {
          return findAgentArticle(
            { articleId, articleLink },
            itemsRef.current,
            {
              getById: async (id) => {
                check()
                return brewApi.getItem(id, undefined, { signal })
              },
              getPage: async (page, perPage) => {
                check()
                return brewApi.getItems(
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
        }
        if (wantsLatestOnly({ articleId, articleLink, openLatest })) {
          const data = await brewApi.getItems(
            { per_page: 1, filter: 'all' },
            undefined,
            { signal },
          )
          return data.items[0]
        }
        return null
      }, { queue })
    }

    console.log('[Brew] Registering agent:open-brew-article event listener')
    window.addEventListener('agent:open-brew-article', handleAgentOpenArticle)

    const takePending = (key: string) => {
      const taken = takeFreshPending<AgentOpenDetail>(
        sessionStorage.getItem(key),
        Date.now(),
      )
      if (taken.ok) {
        sessionStorage.removeItem(key)
        const subject = brewSubject.capture()
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
        console.log(
          key === PENDING_READING_KEY
            ? '[Brew] Pending reading list expired, removing'
            : '[Brew] Pending action expired, removing',
        )
      } else {
        console.error(
          key === PENDING_READING_KEY
            ? '[Brew] Failed to parse pending reading list:'
            : '[Brew] Failed to parse pending action:',
          taken.reason,
        )
      }
      return null
    }

    const pendingReading = takePending(PENDING_READING_KEY)
    if (pendingReading) {
      console.log(
        '[Brew] Found pending reading list from sessionStorage:',
        pendingReading,
      )
      if (pendingReading.readingList) {
        window.dispatchEvent(
          new CustomEvent('agent:set-reading-list', {
            detail: {
              ...pendingReading.readingList,
              createdAt: new Date(pendingReading.readingList.createdAt),
            },
          }),
        )
      }
      if (pendingReading.articleId) {
        timers.push(
          window.setTimeout(() => {
            console.log(
              '[Brew] Opening first article from reading list:',
              pendingReading.articleId,
            )
            void handleAgentOpenArticle(
              new CustomEvent('agent:open-brew-article', {
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
      console.log(
        '[Brew] Found pending action from sessionStorage:',
        pendingOpen,
      )
      timers.push(
        window.setTimeout(() => {
          console.log('[Brew] Executing pending action from sessionStorage')
          void handleAgentOpenArticle(
            new CustomEvent('agent:open-brew-article', {
              detail: pendingOpen,
            }),
          )
        }, 100),
      )
    }

    return () => {
      console.log('[Brew] Unregistering agent:open-brew-article event listener')
      window.removeEventListener(
        'agent:open-brew-article',
        handleAgentOpenArticle,
      )
      for (const id of timers) window.clearTimeout(id)
    }
  }, [])
}
