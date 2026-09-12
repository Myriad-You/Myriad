export {
  applySandboxCapabilityProfile,
  HEADLESS_DENIED_ACTIONS,
  type SandboxCapabilityProfile,
} from './capabilityProfiles'

export {
  getResourceLoader,
  loadPageResources,
  loadWidgetResources,
  type PageResources,
  TappResourceLoader,
  type WidgetResources,
} from './resourceLoader'
export {
  generateFullSDK,
  generateWidgetSDK,
  resolveWidgetSdkCaps,
} from './sdkGenerator'
export type { WidgetSdkCaps } from './sdkGenerator'

export {
  cspOptionsFromPermissions,
  escapeSandboxHtmlText,
  escapeSandboxScriptSource,
  generateCSP,
  type GenerateCSPOptions,
  generateNonce,
  generateSecurityWrapper,
  generateSessionToken,
  IFRAME_SANDBOX_ATTRS,
  sanitizeStorageValue,
  serializeSandboxScriptValue,
  validateStorageKey,
} from './security'

export {
  BASE_CSS,
  generateOnDemandTailwindCSS,
  generateThemeCSS,
  PAGE_CSS,
  PAGE_STATIC_CSS,
  WIDGET_CSS,
  WIDGET_STATIC_CSS,
} from './styles'

export * from './types'
