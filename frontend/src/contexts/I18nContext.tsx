/**
 * 国际化 Context
 * 提供语言切换和翻译功能
 */

import type { Locale, TranslationKeys } from '../i18n'
import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import { getDefaultLocale, saveLocale } from '../i18n'
import { enUS } from '../i18n/en-US'
import { jaJP } from '../i18n/ja-JP'
import { zhCN } from '../i18n/zh-CN'

// 翻译映射
const translations: Record<Locale, TranslationKeys> = {
  'zh-CN': zhCN,
  'en-US': enUS,
  'ja-JP': jaJP,
}

// Context 类型
interface I18nContextType {
  locale: Locale
  setLocale: (locale: Locale) => void
  t: TranslationKeys
  // 便捷方法：格式化带参数的字符串
  format: (template: string, params: Record<string, string | number>) => string
}

const I18nContext = createContext<I18nContextType | null>(null)

// Provider 组件
export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [locale, setLocaleState] = useState<Locale>(getDefaultLocale)

  // 切换语言
  const setLocale = useCallback((newLocale: Locale) => {
    setLocaleState(newLocale)
    saveLocale(newLocale)
    // 更新 HTML lang 属性
    document.documentElement.lang = newLocale
  }, [])

  // 初始化时设置 HTML lang
  useEffect(() => {
    document.documentElement.lang = locale
  }, [locale])

  // 当前翻译
  const t = useMemo(() => translations[locale], [locale])

  // 格式化带参数的字符串，如 "请在 {seconds} 秒后重试"
  const format = useCallback((template: string, params: Record<string, string | number>) => {
    return template.replace(/\{(\w+)\}/g, (_, key) => {
      return String(params[key] ?? `{${key}}`)
    })
  }, [])

  const value = useMemo(() => ({
    locale,
    setLocale,
    t,
    format,
  }), [locale, setLocale, t, format])

  return (
    <I18nContext.Provider value={value}>
      {children}
    </I18nContext.Provider>
  )
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
