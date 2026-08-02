export { buildSearchableContent } from './buildSearchableContent'
export { ConfigNavItem } from './ConfigNavItem'
export {
  CONFIG_NAV_STORAGE_KEY,
  loadConfigNavPersisted,
  resolveInitialConfigSection,
  saveConfigNavPersisted,
  snapshotConfigNavScroll,
} from './configNavPersistence'
export {
  defaultAiFieldValue,
  defaultUiFieldValue,
  mapConfigFields,
} from './defaultFieldValues'
export {
  DEFAULT_AUTO_FETCH_CONFIG,
  DEFAULT_CONFIG_FAVORITES,
  DEFAULT_PERMISSION_CONFIG,
  LEGACY_CONFIG_SECTION_MAP,
  loadConfigFavorites,
} from './defaults'
/**
 * ConfigForm 拆分模块：类型 / 默认值 / 搜索 / 旁路 draft / 保存 / 重置 / 导航
 */
export type {
  AiConfig,
  Config,
  ConfigField,
  PlatformConfig,
  QuickAccessItem,
  ReportConfig,
  SaveLibrarySourcePreferencesResponse,
  ShowMessage,
  UiConfig,
} from './types'
export { useConfigBagState } from './useConfigBagState'
export { useConfigDirty } from './useConfigDirty'
export { useConfigMessage } from './useConfigMessage'
export { useConfigNavigation } from './useConfigNavigation'
export { useConfigReset } from './useConfigReset'
export { useConfigSave } from './useConfigSave'
export { useConfigSearch } from './useConfigSearch'
export { useConfigSideDrafts } from './useConfigSideDrafts'
