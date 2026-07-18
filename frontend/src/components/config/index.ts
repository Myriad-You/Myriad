/**
 * 配置区块组件统一导出
 */

export { default as AboutConfigSection } from './AboutConfigSection'
export { AdvancedConfigSection } from './AdvancedConfigSection'
export { default as AiConfigSection } from './AiConfigSection'
export {
  areLibrarySourcePreferencesEqual,
  DEFAULT_LIBRARY_SOURCE_PREFERENCES,
  default as ModuleConfigSection,
  normalizeLibraryPreferences,
} from './ModuleConfigSection'
export type { LibrarySourcePreferences } from './ModuleConfigSection'
export { default as MusicConfigSection } from './MusicConfigSection'
export { default as NetworkConfigSection } from './NetworkConfigSection'
export { default as NotificationConfigSection } from './NotificationConfigSection'
export { default as OAuthConfigSection } from './OAuthConfigSection'
export { default as PermissionsConfigSection } from './PermissionsConfigSection'
export { default as PlatformAutoRefreshSettings } from './PlatformAutoRefreshSettings'
export type { PlatformAutoFetchConfig } from './PlatformAutoRefreshSettings'
export { default as UiConfigSection } from './UiConfigSection'
export { UpdaterInlinePanel } from './UpdaterConfigSection'
export { default as UsersConfigSection } from './UsersConfigSection'
