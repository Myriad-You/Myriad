import type { Locale } from './index'

export type LocaleImporters<T> = { [L in Locale]: () => Promise<T> }

/** Cache + in-flight dedupe. Unused locales stay out of the main bundle. */
/* @__NO_SIDE_EFFECTS__ */
export function createLocaleLoader<T>(importers: LocaleImporters<T>) {
  const cache = new Map<Locale, T>()
  const failures = new Map<Locale, unknown>()
  const inflight = new Map<Locale, Promise<T>>()

  function load(locale: Locale): Promise<T> {
    const cached = cache.get(locale)
    if (cached) return Promise.resolve(cached)

    const pending = inflight.get(locale)
    if (pending) return pending

    failures.delete(locale)
    const promise = Promise.try(async () => {
      try {
        const value = await importers[locale]()
        cache.set(locale, value)
        return value
      } catch (error) {
        failures.set(locale, error)
        throw error
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

  /** Only call during React render; event handlers use load() to retry explicitly. */
  function read(locale: Locale): T {
    const cached = cache.get(locale)
    if (cached) return cached
    if (failures.has(locale)) throw failures.get(locale)
    throw load(locale)
  }

  return { load, getCached, seed, read }
}
