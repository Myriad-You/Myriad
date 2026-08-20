import type React from 'react'
import type { PlatformAutoFetchConfig } from '../PlatformAutoRefreshSettings'

export interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

export interface PlatformConfig {
  name: string
  enabled: boolean
  has_token: boolean
  config_fields: ConfigField[]
  description: string
  icon: string
}

export interface AiConfig {
  config_fields: ConfigField[]
}

export interface TripoConfig {
  config_fields: ConfigField[]
}

export interface ReportConfig {
  config_fields: ConfigField[]
}

/** 管理端 ui_config：仅 bag */
export interface UiConfig {
  config_fields: ConfigField[]
}

export interface Config {
  platforms: PlatformConfig[]
  auto_fetch: PlatformAutoFetchConfig
  ai_config: AiConfig
  tripo_config: TripoConfig
  report_config: ReportConfig
  ui_config: UiConfig
}

export interface QuickAccessItem {
  id: string
  label: string
  /** Page header description (source of truth; not from search aliases). */
  description: string
  icon: React.ReactNode
  section: string
  subsection?: string
}

export interface SaveLibrarySourcePreferencesResponse {
  success: boolean
  preferences?: import('../ModuleConfigSection').LibrarySourcePreferences
  message?: string
}

export type ShowMessage = (
  message: string,
  type?: import('../../Toast').ToastType,
  duration?: number,
) => void
