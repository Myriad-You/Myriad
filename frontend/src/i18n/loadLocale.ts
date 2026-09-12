import type { Locale, TranslationKeys } from './index'
import { assembleLocale } from './assembleLocale'
import { createLocaleLoader } from './createLocaleLoader'

// Vite serves `*.json?import` as JS. A JSON import attribute makes the
// browser require application/json and reject the module.

async function loadPack(locale: Locale): Promise<TranslationKeys> {
  switch (locale) {
    case 'zh-CN': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./zh-CN.json'),
        import('./config.zh-CN.json'),
        import('./tapp.zh-CN.json'),
        import('./brew.zh-CN.json'),
        import('./merope.zh-CN.json'),
        import('./errors.zh-CN.json'),
        import('./agentCaps.zh-CN.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    case 'en-US': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./en-US.json'),
        import('./config.en-US.json'),
        import('./tapp.en-US.json'),
        import('./brew.en-US.json'),
        import('./merope.en-US.json'),
        import('./errors.en-US.json'),
        import('./agentCaps.en-US.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    case 'ja-JP': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./ja-JP.json'),
        import('./config.ja-JP.json'),
        import('./tapp.ja-JP.json'),
        import('./brew.ja-JP.json'),
        import('./merope.ja-JP.json'),
        import('./errors.ja-JP.json'),
        import('./agentCaps.ja-JP.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    case 'zh-TW': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./zh-TW.json'),
        import('./config.zh-TW.json'),
        import('./tapp.zh-TW.json'),
        import('./brew.zh-TW.json'),
        import('./merope.zh-TW.json'),
        import('./errors.zh-TW.json'),
        import('./agentCaps.zh-TW.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    case 'ko-KR': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./ko-KR.json'),
        import('./config.ko-KR.json'),
        import('./tapp.ko-KR.json'),
        import('./brew.ko-KR.json'),
        import('./merope.ko-KR.json'),
        import('./errors.ko-KR.json'),
        import('./agentCaps.ko-KR.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    case 'fr-FR': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./fr-FR.json'),
        import('./config.fr-FR.json'),
        import('./tapp.fr-FR.json'),
        import('./brew.fr-FR.json'),
        import('./merope.fr-FR.json'),
        import('./errors.fr-FR.json'),
        import('./agentCaps.fr-FR.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    case 'de-DE': {
      const [core, config, tapp, brew, merope, errors, agentCaps] = await Promise.all([
        import('./de-DE.json'),
        import('./config.de-DE.json'),
        import('./tapp.de-DE.json'),
        import('./brew.de-DE.json'),
        import('./merope.de-DE.json'),
        import('./errors.de-DE.json'),
        import('./agentCaps.de-DE.json'),
      ])
      return assembleLocale(core.default, {
        config: config.default,
        tapp: tapp.default,
        brew: brew.default,
        merope: merope.default,
        errors: errors.default,
        agentCaps: agentCaps.default,
      })
    }
    default: {
      const _exhaustive: never = locale
      throw new Error(`Unknown locale: ${_exhaustive}`)
    }
  }
}

const loader = createLocaleLoader<TranslationKeys>({
  'zh-CN': () => loadPack('zh-CN'),
  'zh-TW': () => loadPack('zh-TW'),
  'en-US': () => loadPack('en-US'),
  'ja-JP': () => loadPack('ja-JP'),
  'ko-KR': () => loadPack('ko-KR'),
  'fr-FR': () => loadPack('fr-FR'),
  'de-DE': () => loadPack('de-DE'),
})

/** Cache + in-flight dedupe; unused locales stay out of the main bundle. */
export function loadLocale(locale: Locale): Promise<TranslationKeys> {
  return loader.load(locale)
}

export function getCachedLocale(locale: Locale): TranslationKeys | null {
  return loader.getCached(locale)
}
