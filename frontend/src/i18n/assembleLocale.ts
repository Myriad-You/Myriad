export type LocaleCore = typeof import('./en-US.json')
export type LocaleConfig = typeof import('./config.en-US.json')
export type LocaleTapp = typeof import('./tapp.en-US.json')
export type LocaleBrew = typeof import('./brew.en-US.json')
export type LocaleMerope = typeof import('./merope.en-US.json')
export type LocaleErrors = typeof import('./errors.en-US.json')
export type LocaleAgentCaps = typeof import('./agentCaps.en-US.json')

export type TranslationKeys = LocaleCore & {
  config: LocaleConfig
  tapp: LocaleTapp
  brew: LocaleBrew
  merope: LocaleMerope
  errors: LocaleErrors
  agentCaps: LocaleAgentCaps
}

export function assembleLocale(
  core: LocaleCore,
  namespaces: {
    config: LocaleConfig
    tapp: LocaleTapp
    brew: LocaleBrew
    merope: LocaleMerope
    errors: LocaleErrors
    agentCaps: LocaleAgentCaps
  },
): TranslationKeys {
  return {
    ...core,
    ...namespaces,
  }
}
