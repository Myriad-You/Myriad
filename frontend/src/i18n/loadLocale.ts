/**
 * 按需加载语言包
 * - 同一语言只加载一次并缓存
 * - 非当前语言不会进入主包，降低常驻 JS 堆
 */

import type { Locale, TranslationKeys } from './index'

const cache = new Map<Locale, TranslationKeys>()
const inflight = new Map<Locale, Promise<TranslationKeys>>()

async function importLocale(locale: Locale): Promise<TranslationKeys> {
  switch (locale) {
    case 'zh-CN':
      return (await import('./zh-CN')).zhCN
    case 'en-US':
      return (await import('./en-US')).enUS
    case 'ja-JP':
      return (await import('./ja-JP')).jaJP
    default: {
      const _exhaustive: never = locale
      throw new Error(`Unknown locale: ${_exhaustive}`)
    }
  }
}

/**
 * 加载指定语言翻译（带内存缓存与 in-flight 去重）
 */
export function loadLocale(locale: Locale): Promise<TranslationKeys> {
  const cached = cache.get(locale)
  if (cached) return Promise.resolve(cached)

  const pending = inflight.get(locale)
  if (pending) return pending

  const promise = importLocale(locale)
    .then((t) => {
      cache.set(locale, t)
      inflight.delete(locale)
      return t
    })
    .catch((err) => {
      inflight.delete(locale)
      throw err
    })

  inflight.set(locale, promise)
  return promise
}

/**
 * 同步读取已缓存的语言包（未加载返回 null）
 */
export function getCachedLocale(locale: Locale): TranslationKeys | null {
  return cache.get(locale) ?? null
}
