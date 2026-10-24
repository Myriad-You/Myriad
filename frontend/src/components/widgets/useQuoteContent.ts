import type { QuoteData } from '../../utils/dynamicContent'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useVisibilityInterval } from '../../hooks/animation'
import { clearQuoteContentCache, getRandomQuote, HITOKOTO_CONFIG_UPDATED_EVENT } from '../../utils/dynamicContent'
import { formatUserFacingError } from '../../utils/formatUserFacingError'

const CACHE_KEY = 'quote_data_cache'
const CACHE_DURATION = 60 * 60 * 1000

/** Own quote refreshes for this mounted consumer, including their cache writes. */
export function useQuoteContent(isPreview = false) {
  const { t, locale } = useI18n()
  const [quoteData, setQuoteData] = useState<QuoteData | null>(null)
  const [loading, setLoading] = useState(true)
  const [fetchError, setFetchError] = useState('')
  const requestRef = useRef<AbortController | null>(null)

  const loadFromCache = useCallback(() => {
    try {
      const cached = localStorage.getItem(CACHE_KEY)
      if (cached) {
        const { data, timestamp } = JSON.parse(cached)
        if (Date.now() - timestamp < CACHE_DURATION) {
          setQuoteData(data)
          return true
        }
      }
    } catch (err) {
      console.error(`${t.quoteWidget.loadCacheFailed}:`, err)
    }
    return false
  }, [t])

  const saveToCache = useCallback(
    (data: QuoteData) => {
      try {
        localStorage.setItem(
          CACHE_KEY,
          JSON.stringify({
            data,
            timestamp: Date.now(),
          }),
        )
      } catch (err) {
        console.error(`${t.quoteWidget.saveCacheFailed}:`, err)
      }
    },
    [t],
  )

  const fetchQuote = useCallback(async () => {
    requestRef.current?.abort()
    const request = new AbortController()
    requestRef.current = request
    try {
      const quote = await getRandomQuote(locale, request.signal)
      if (request.signal.aborted) return
      if (quote) {
        setQuoteData(quote)
        setFetchError('')
        saveToCache(quote)
      }
    } catch (error) {
      if (request.signal.aborted) return
      console.error(`${t.quoteWidget.fetchQuoteFailed}:`, error)
      // 错误码表只在出错时才拉，不压在每张卡片的首屏上（博客里每个小组件各启动一次）。
      const message = await formatUserFacingError(error, t.quoteWidget.fetchQuoteFailed)
      if (!request.signal.aborted) setFetchError(message)
    } finally {
      if (!request.signal.aborted) setLoading(false)
    }
  }, [saveToCache, t, locale])

  useEffect(() => {
    if (isPreview) {
      setQuoteData({
        text: t.quoteWidget.defaultQuote,
        author: t.quoteWidget.anonymous,
      })
      setLoading(false)
      return
    }

    const hasCache = loadFromCache()
    if (hasCache) {
      setLoading(false)
    }

    void fetchQuote()
    return () => requestRef.current?.abort()
  }, [
    loadFromCache,
    fetchQuote,
    isPreview,
    t.quoteWidget.defaultQuote,
    t.quoteWidget.anonymous,
  ])

  useEffect(() => {
    if (isPreview) return
    const onConfigUpdated = () => {
      clearQuoteContentCache()
      void fetchQuote()
    }
    window.addEventListener(HITOKOTO_CONFIG_UPDATED_EVENT, onConfigUpdated)
    return () => {
      window.removeEventListener(
        HITOKOTO_CONFIG_UPDATED_EVENT,
        onConfigUpdated,
      )
    }
  }, [fetchQuote, isPreview])

  useVisibilityInterval(fetchQuote, { delay: CACHE_DURATION, enabled: !isPreview })

  return { quoteData, loading, fetchError }
}
