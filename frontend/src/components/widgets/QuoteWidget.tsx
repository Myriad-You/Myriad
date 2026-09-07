/**
 * 一言小组件
 * 现代化Glass风格设计
 */

import type { QuoteData } from '../../utils/dynamicContent'

import type { WidgetConfig } from '../widgetGridTypes'
import { motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useHomeVisibilityInterval } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import {
  getRandomQuote,
  HITOKOTO_CONFIG_UPDATED_EVENT,
} from '../../utils/dynamicContent'
import { userFacingError } from '../../utils/userFacingError'
import { Spinner } from '../Spinner'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'

// 缓存配置
const CACHE_KEY = 'quote_data_cache'
const CACHE_DURATION = 60 * 60 * 1000 // 1小时

export interface QuoteWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
}

export const QuoteWidget = memo(
  ({ config, isEditMode: _isEditMode, isPreview }: QuoteWidgetProps) => {
    const { containerRef, scale, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t, locale } = useI18n()
    const [quoteData, setQuoteData] = useState<QuoteData | null>(null)
    const [loading, setLoading] = useState(true)
    const [fetchError, setFetchError] = useState('')
    const [themeColor, setThemeColor] = useState('#a855f7')

    // 从缓存加载
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

    // 保存到缓存
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
      try {
        // Pass host UI locale so local fallback + source pick match language
        const quote = await getRandomQuote(locale)
        if (quote) {
          setQuoteData(quote)
          setFetchError('')
          saveToCache(quote)
        }
      } catch (error) {
        console.error(`${t.quoteWidget.fetchQuoteFailed}:`, error)
        setFetchError(
          userFacingError(error, t.quoteWidget.fetchQuoteFailed),
        )
      } finally {
        setLoading(false)
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

      // 先尝试从缓存加载
      const hasCache = loadFromCache()
      if (hasCache) {
        setLoading(false)
      }

      // 然后获取最新一言
      fetchQuote()
    }, [
      loadFromCache,
      fetchQuote,
      isPreview,
      t.quoteWidget.defaultQuote,
      t.quoteWidget.anonymous,
    ])

    // Config save (hitokoto source change) → drop local caches and refetch
    useEffect(() => {
      if (isPreview) return
      const onConfigUpdated = () => {
        try {
          localStorage.removeItem(CACHE_KEY)
          localStorage.removeItem('quote_cache')
          localStorage.removeItem('quote_cache_time')
          localStorage.removeItem('quote_cache_source')
        } catch {
          /* ignore */
        }
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

    // 使用首页原子化可见性感知定时器，页面隐藏时自动暂停
    useHomeVisibilityInterval(fetchQuote, CACHE_DURATION, !isPreview)

    // 主题色获取 - 优化：使用 requestAnimationFrame 批处理避免强制重排
    const updateThemeColor = useCallback(() => {
      // 使用 requestAnimationFrame 延迟读取，避免同步强制重排
      requestAnimationFrame(() => {
        const primaryColor =
          getComputedStyle(document.documentElement)
            .getPropertyValue('--color-primary')
            .trim() || '#a855f7'
        setThemeColor((prev) => (prev !== primaryColor ? primaryColor : prev))
      })
    }, [])

    useEffect(() => {
      updateThemeColor()
      // 监听主题色变化 - 使用节流
      let throttleTimer: ReturnType<typeof setTimeout> | null = null
      const throttledUpdate = () => {
        if (throttleTimer) return
        throttleTimer = setTimeout(() => {
          throttleTimer = null
          updateThemeColor()
        }, 300) // 增加节流时间到 300ms
      }

      const observer = new MutationObserver(throttledUpdate)
      observer.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ['style'],
      })

      return () => {
        if (throttleTimer) clearTimeout(throttleTimer)
        observer.disconnect()
      }
    }, [updateThemeColor])

    if (loading) {
      return (
        <div className="h-full w-full flex items-center justify-center">
          <Spinner size="lg" color="primary" />
        </div>
      )
    }

    if (!quoteData) {
      return (
        <div className="h-full w-full flex items-center justify-center text-gray-400">
          <span>
            {fetchError || t.quoteWidget.unavailable}
          </span>
        </div>
      )
    }

    // 4x2 宽版布局 - 上下结构重构
    if (config.size === '4x2') {
      return (
        <WidgetShell
          containerRef={containerRef}
          padding={20}
          contentClassName="flex flex-col"
          background={
            <>
              <GlowBackground
                color={themeColor}
                animLevel={anim.level}
                shouldAnimate={anim.loop}
                variant="single-left"
                size="lg"
                opacity={0.2}
              />
              <div className="absolute top-2 left-4 text-8xl opacity-[0.08] font-serif text-gray-500 leading-none select-none pointer-events-none">
                "
              </div>
            </>
          }
        >
          {/* 上半部分：引言内容 (占据主要空间) */}
          <div className="flex-1 flex items-center justify-center relative z-10 w-full min-h-0 overflow-hidden">
            <motion.p
              className="text-lg font-medium text-gray-800 dark:text-gray-100 leading-relaxed font-serif italic text-center w-full line-clamp-2 transition-all duration-300 ease-out"
              style={{
                fontSize: `${18 * fontScale}px`,
                textShadow: '0 2px 10px rgba(0,0,0,0.05)',
              }}
              initial={{ scale: 0.95, opacity: 0, y: 5 }}
              animate={{ scale: 1, opacity: 1, y: 0 }}
              transition={{ duration: 0.5 }}
            >
              {quoteData.text}
            </motion.p>
          </div>

          {/* 下半部分：作者信息 (底部右侧) */}
          {quoteData.author && (
            <motion.div
              className="shrink-0 flex items-center justify-end gap-3 pt-3 mt-1 border-t border-gray-200/10 dark:border-white/5 w-full"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              transition={{ delay: 0.3 }}
            >
              <div className="h-px w-16 bg-linear-to-r from-transparent to-gray-400/40"></div>
              <span
                className="text-sm text-gray-500 dark:text-gray-400 font-medium tracking-widest uppercase opacity-80 truncate max-w-[60%]"
                style={{ fontSize: `${12 * fontScale}px` }}
              >
                {quoteData.author}
              </span>
            </motion.div>
          )}
        </WidgetShell>
      )
    }

    // 4x1 紧凑横版布局
    if (config.size === '4x1') {
      return (
        <WidgetShell
          containerRef={containerRef}
          padding={{ x: 16, y: 6 }}
          contentClassName="flex items-center"
          background={
            <GlowBackground
              color={themeColor}
              animLevel={anim.level}
              shouldAnimate={false}
              variant="single-left"
              size="sm"
              opacity={0.3}
            />
          }
        >
          {/* 引言内容 - 增加右侧内边距避开作者信息 */}
          <motion.p
            className="text-sm font-medium text-gray-800 dark:text-gray-100 leading-relaxed font-serif italic w-full pr-12 line-clamp-2"
            style={{ fontSize: `${14 * fontScale}px` }}
            initial={{ opacity: 0, x: -10 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.5 }}
          >
            {quoteData.text}
          </motion.p>

          {/* 作者信息 - 绝对定位在右下角 */}
          {quoteData.author && (
            <motion.div
              className="absolute bottom-1.5 right-3 z-10 max-w-[40%] truncate"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ delay: 0.2 }}
            >
              <span
                className="text-[10px] text-gray-500 dark:text-gray-400 font-medium"
                style={{ fontSize: `${9 * fontScale}px` }}
              >
                — {quoteData.author}
              </span>
            </motion.div>
          )}
        </WidgetShell>
      )
    }

    return (
      <WidgetShell
        containerRef={containerRef}
        scale={scale}
        padding={12}
        contentClassName="flex flex-col"
        background={
          <GlowBackground
            color={themeColor}
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="md"
          />
        }
      >
        {/* 顶部：图标 */}
        <motion.div
          className="mb-1"
          initial={{ scale: 0.5, opacity: 0, rotate: -15 }}
          animate={{
            scale: 1,
            opacity: 1,
            rotate: 0,
          }}
          transition={{
            duration: 0.6,
            ease: [0.34, 1.56, 0.64, 1],
          }}
        >
          <svg
            className="w-6 h-6"
            style={{
              color: themeColor,
              width: `${24 * scale}px`,
              height: `${24 * scale}px`,
            }}
            fill="currentColor"
            viewBox="0 0 24 24"
          >
            <path d="M6 17h3l2-4V7H5v6h3zm8 0h3l2-4V7h-6v6h3z" />
          </svg>
        </motion.div>

        {/* 中部：引言内容 */}
        <div className="flex-1 flex flex-col justify-center min-h-0">
          <motion.p
            className="text-sm font-medium text-gray-800 dark:text-gray-100 leading-relaxed line-clamp-3"
            style={{ fontSize: `${14 * fontScale}px`, lineHeight: 1.6 }}
            initial={{ x: -20, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            transition={{
              duration: 0.6,
              delay: 0.2,
              ease: [0.34, 1.56, 0.64, 1],
            }}
          >
            {quoteData.text}
          </motion.p>
        </div>

        {/* 底部：作者信息 */}
        {quoteData.author && (
          <motion.div
            className="text-[10px] text-gray-500 dark:text-gray-500 text-right"
            style={{ fontSize: `${10 * fontScale}px` }}
            initial={{ y: 10, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.3 }}
          >
            — {quoteData.author}
          </motion.div>
        )}
      </WidgetShell>
    )
  },
)

QuoteWidget.displayName = 'QuoteWidget'
