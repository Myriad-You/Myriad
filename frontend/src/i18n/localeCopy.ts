import type { LocaleCore, ShellChromeKeys, ShellTranslationKeys } from './assembleLocale'
import type { Locale } from './locales'
import config from './configService.en-US.json' with { type: 'json' }
import errors from './errors.en-US.json' with { type: 'json' }
import { formatMessage } from './formatMessage'
import {
  getCachedShellLocale,
  getCachedShellNamespace,
  loadShellLocale,
} from './loadLocale'
import { getDefaultLocale, localeOrFallback } from './locales'
import service from './service.en-US.json' with { type: 'json' }

/** Service callers only need settings errors, not the settings editor catalog. */
export type ServiceCopy = ShellTranslationKeys

let enCore: LocaleCore | null = null
let enCorePromise: Promise<LocaleCore> | null = null

/** Tests install the English chrome pack synchronously. Production loads it on demand. */
export function installEnglishCoreFallback(core: LocaleCore): void {
  enCore = core
}

function loadEnglishCore(): Promise<LocaleCore> {
  enCorePromise ??= import('./en-US.json').then((module) => {
    enCore = module.default
    return module.default
  })
  return enCorePromise
}

function englishChrome(): ShellChromeKeys {
  if (enCore) {
    return { ...enCore, config, errors }
  }
  const cached = getCachedShellLocale('en-US')
  if (cached) return cached
  void loadEnglishCore()
  return { ...(enCore ?? ({} as LocaleCore)), config, errors }
}

function asLocale(value: string) {
  return localeOrFallback(value)
}

function assembleServiceCopy(
  locale: Locale,
  chrome: ShellChromeKeys,
): ServiceCopy {
  const tapp =
    getCachedShellNamespace('tapp', locale) ??
    getCachedShellNamespace('tapp', 'en-US') ??
    service.tapp
  const phantasi =
    getCachedShellNamespace('phantasi', locale) ??
    getCachedShellNamespace('phantasi', 'en-US') ??
    service.phantasi
  const merope =
    getCachedShellNamespace('merope', locale) ??
    getCachedShellNamespace('merope', 'en-US') ??
    service.merope
  const agentCaps =
    getCachedShellNamespace('agentCaps', locale) ??
    getCachedShellNamespace('agentCaps', 'en-US') ??
    {}
  return {
    ...chrome,
    tapp,
    phantasi,
    merope,
    agentCaps,
  } as ShellTranslationKeys
}

function resolveServiceCopy(): { locale: Locale; t: ServiceCopy } {
  const target = getDefaultLocale()
  const cached = getCachedShellLocale(target)
  if (cached) return { locale: target, t: assembleServiceCopy(target, cached) }
  void loadShellLocale(target).catch(() => {})
  const loaded = getCachedShellLocale(target)
  if (loaded) return { locale: target, t: assembleServiceCopy(target, loaded) }
  const en = getCachedShellLocale('en-US')
  return { locale: 'en-US', t: assembleServiceCopy('en-US', en ?? englishChrome()) }
}

if (getDefaultLocale() === 'en-US') {
  void loadEnglishCore()
}

/** ja/zh stay out of the static graph; English chrome loads on demand. */
export function copyForLocale(locale: string): ServiceCopy {
  const key = asLocale(locale)
  const cached = getCachedShellLocale(key)
  if (cached) return assembleServiceCopy(key, cached)
  void loadShellLocale(key).catch(() => {})
  const loaded = getCachedShellLocale(key)
  if (loaded) return assembleServiceCopy(key, loaded)
  const en = getCachedShellLocale('en-US')
  return assembleServiceCopy('en-US', en ?? englishChrome())
}

/** Non-React service-layer copy. Same pack `formatCurrent` formats against. */
export function currentCopy(): ServiceCopy {
  return resolveServiceCopy().t
}

/** ICU against the catalog `currentCopy()` actually returned, not the in-flight target. */
export function formatCurrent(
  template: string,
  params: Record<string, string | number> = {},
): string {
  return formatMessage(resolveServiceCopy().locale, template, params)
}
