/**
 * 国际化 Context
 * 提供语言切换和翻译功能
 *
 * 内存优化：语言包按需动态加载，同一时间只强制常驻当前语言；
 * 切换时保留旧文案直到新包就绪，避免闪空白或闪错语言。
 */

import type { Locale, TranslationKeys } from '../i18n'
import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { getDefaultLocale, saveLocale } from '../i18n'
import {
  getCachedLocale,
  loadLocale,
} from '../i18n/loadLocale'
import { currentCopy } from '../i18n/localeCopy'

// Context 类型
interface I18nContextType {
  locale: Locale
  setLocale: (locale: Locale) => void
  t: TranslationKeys
  // 便捷方法：格式化带参数的字符串
  format: (template: string, params: Record<string, string | number>) => string
}

const I18nContext = createContext<I18nContextType | null>(null)

interface LocaleBundle {
  locale: Locale
  t: TranslationKeys
}

// Provider 组件
export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [locale, setLocaleState] = useState<Locale>(getDefaultLocale)
  // bundle 的 locale 与 t 原子更新，避免「语言已切、文案未到」
  const [bundle, setBundle] = useState<LocaleBundle | null>(() => {
    const initial = getDefaultLocale()
    const cached = getCachedLocale(initial)
    return cached ? { locale: initial, t: cached } : null
  })

  // 加载 / 切换语言包
  useEffect(() => {
    let cancelled = false

    // 已是目标语言且有包，跳过
    if (bundle?.locale === locale) return

    loadLocale(locale)
      .then((t) => {
        if (cancelled) return
        setBundle({ locale, t })
      })
      .catch((err) => {
        console.error('[I18n] Failed to load locale:', locale, err)
        if (locale !== 'en-US') {
          void import('../utils/toastManager').then(({ showError }) => {
            showError(currentCopy().errors.localeLoadFailed)
          })
          loadLocale('en-US').then((t) => {
            if (cancelled) return
            setLocaleState('en-US')
            setBundle({ locale: 'en-US', t })
          })
        }
      })

    return () => {
      cancelled = true
    }
  }, [locale, bundle?.locale])

  // 切换语言：先改目标 locale，文案等包就绪后与 bundle 一并切换（无闪断）
  const setLocale = useCallback((newLocale: Locale) => {
    setLocaleState(newLocale)
    saveLocale(newLocale)
    void import('../utils/analyticsEvents').then(
      ({ trackProductEvent, AnalyticsEvents }) => {
        trackProductEvent(AnalyticsEvents.LOCALE_SWITCH, {
          target: newLocale,
          throttleMs: 3000,
        })
      },
    )
  }, [])

  // HTML lang 与「已生效」的语言包对齐，避免文案未到却改了 lang
  useEffect(() => {
    if (bundle) {
      document.documentElement.lang = bundle.locale
    }
  }, [bundle])

  // 格式化带参数的字符串，如 "请在 {seconds} 秒后重试"
  const format = useCallback(
    (template: string, params: Record<string, string | number>) => {
      return template.replace(/\{(\w+)\}/g, (_, key) => {
        return String(params[key] ?? `{${key}}`)
      })
    },
    [],
  )

  const value = useMemo(() => {
    if (!bundle) return null
    return {
      locale: bundle.locale,
      setLocale,
      t: bundle.t,
      format,
    }
  }, [bundle, setLocale, format])

  // 首包未就绪：不挂载子树（外层仍有 PageLoader 遮罩，体验与原先等 JS 解析三语等价/更快）
  if (!value) {
    return null
  }

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

// Hook
export function useI18n(): I18nContextType {
  const context = useContext(I18nContext)
  if (!context) {
    throw new Error('useI18n must be used within an I18nProvider')
  }
  return context
}

// 导出类型
export type { Locale, TranslationKeys }
