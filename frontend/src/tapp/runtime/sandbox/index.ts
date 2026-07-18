/**
 * 沙箱核心模块索引
 *
 * 导出所有原子化模块
 *
 * 🎯 性能优化：
 * - 导出预计算的组合 CSS
 * - 导出缓存管理函数
 * - 导出分离式资源加载器
 */

export {
  applySandboxCapabilityProfile,
  HEADLESS_DENIED_ACTIONS,
  type SandboxCapabilityProfile,
} from './capabilityProfiles'

// 🎯 资源加载器（分离式 CSS 处理）
export {
  getResourceLoader,
  loadPageResources,
  loadWidgetResources,
  type PageResources,
  TappResourceLoader,
  type WidgetResources,
} from './resourceLoader'
// SDK 生成器
export { generateFullSDK, generateWidgetSDK } from './sdkGenerator'

// 安全策略
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

// 样式（含性能优化的预计算 CSS）
export {
  BASE_CSS,
  generateOnDemandTailwindCSS,
  generateThemeCSS,
  PAGE_CSS,
  PAGE_STATIC_CSS,
  WIDGET_CSS,
  // 🎯 预计算的组合 CSS
  WIDGET_STATIC_CSS,
} from './styles'

// 类型
export * from './types'
