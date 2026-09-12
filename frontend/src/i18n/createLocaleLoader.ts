import type { Locale } from './index'

export type LocaleImporters<T> = { [L in Locale]: () => Promise<T> }

/** Cache + in-flight dedupe. Unused locales stay out of the main bundle. */
export function createLocaleLoader<T>(importers: LocaleImporters<T>) {
  const cache = new Map<Locale, T>()
  const inflight = new Map<Locale, Promise<T>>()

  function load(locale: Locale): Promise<T> {
    const cached = cache.get(locale)
    if (cached) return Promise.resolve(cached)

    const pending = inflight.get(locale)
    if (pending) return pending

    const promise = Promise.try(async () => {
      try {
        const value = await importers[locale]()
        cache.set(locale, value)
        return value
      } finally {
        inflight.delete(locale)
      }
    })

    inflight.set(locale, promise)
    return promise
  }

  function getCached(locale: Locale): T | null {
    return cache.get(locale) ?? null
  }

  function seed(locale: Locale, value: T): void {
    cache.set(locale, value)
  }

  return { load, getCached, seed }
}
