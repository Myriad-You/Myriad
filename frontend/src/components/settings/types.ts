import type { CSSProperties, ReactNode } from 'react'
import type { ToggleSwitchPreview } from './items/toggleSwitchPreview'

export type SettingType =
  | 'switch'
  | 'input'
  | 'number'
  | 'slider'
  | 'select'
  | 'provider'
  | 'button'
  | 'checkbox'
  | 'custom'

export type SettingSize = 'sm' | 'md' | 'lg'

export type SettingLayout = 'horizontal' | 'vertical'

export interface SettingOption<T = string> {
  value: T
  label: string
  icon?: ReactNode | string
  badge?: string
  description?: string
  disabled?: boolean
}

export interface BaseSettingItemConfig {
  /** DOM id/name; not React key */
  itemKey?: string
  label: string
  detail?: ReactNode
  guide?: ReactNode
  guidePath?: string
  description?: string
  hint?: string
  required?: boolean
  disabled?: boolean
  loading?: boolean
  error?: string
  size?: SettingSize
  layout?: SettingLayout
  style?: CSSProperties
  className?: string
}

export interface SwitchSettingConfig extends BaseSettingItemConfig {
  type: 'switch'
  value: boolean
  onChange: (value: boolean) => void
  preview?: ToggleSwitchPreview
}

export interface CheckboxSettingConfig extends BaseSettingItemConfig {
  type: 'checkbox'
  value: boolean
  onChange: (value: boolean) => void
  checkboxLabel?: string
}

export type InputItemVariant = 'default' | 'clickToEdit' | 'imageUpload'

export interface InputSettingConfig extends BaseSettingItemConfig {
  type: 'input'
  value: string
  onChange: (value: string) => void
  onFocus?: () => void
  onBlur?: () => void
  placeholder?: string
  inputType?: 'text' | 'password' | 'url' | 'email' | 'search'
  multiline?: boolean
  rows?: number
  autoComplete?: string
  /** select-all on password mask */
  autoSelectOnMask?: boolean
  validate?: (value: string) => string | null
  copyable?: boolean
  variant?: InputItemVariant
  emptyLabel?: string
  editLabel?: string
  saveLabel?: string
  cancelLabel?: string
  /** Promise: loading; reject keeps edit mode */
  onCommit?: (next: string) => void | Promise<void>
  onEditStart?: () => void
  onEditCancel?: () => void
  /** default `image/*` */
  accept?: string
  /** default 512KB */
  maxImageBytes?: number
  uploadLabel?: string
  clearImageLabel?: string
  localImageLabel?: string
  previewAlt?: string
  clearable?: boolean
  imageTypeError?: string
  imageSizeError?: string
  imageReadError?: string
}

export interface NumberSettingConfig extends BaseSettingItemConfig {
  type: 'number'
  value: number
  onChange: (value: number) => void
  onBlur?: () => void
  min?: number
  max?: number
  step?: number
  unit?: string
}

export interface SliderSettingConfig extends BaseSettingItemConfig {
  type: 'slider'
  value: number
  onChange: (value: number) => void
  onBlur?: () => void
  min?: number
  max?: number
  step?: number
  unit?: string
  showValue?: boolean
  formatValue?: (value: number) => string
  showRangeLabels?: boolean
  /** implies range labels */
  startLabel?: string
  /** implies range labels */
  endLabel?: string
  /** hidden if outside min/max */
  recommendedValue?: number
  recommendedLabel?: string
}

export interface SelectSettingConfig<T = string> extends BaseSettingItemConfig {
  type: 'select'
  value: T
  onChange: (value: T) => void
  options: SettingOption<T>[]
}

export interface ProviderSettingConfig<
  T = string,
> extends BaseSettingItemConfig {
  type: 'provider'
  value: T
  onChange: (value: T) => void
  options: SettingOption<T>[]
  /** re-click selected → ''; default true */
  allowDeselect?: boolean
}

export interface ButtonSettingConfig extends Omit<
  BaseSettingItemConfig,
  'label'
> {
  type: 'button'
  label?: string
  onClick: () => void
  buttonText: string
  buttonIcon?: ReactNode | string
  variant?: 'primary' | 'secondary' | 'danger'
}

export interface CustomSettingConfig extends BaseSettingItemConfig {
  type: 'custom'
  render: () => ReactNode
}

export type SettingItemConfig =
  | SwitchSettingConfig
  | CheckboxSettingConfig
  | InputSettingConfig
  | NumberSettingConfig
  | SliderSettingConfig
  | SelectSettingConfig
  | ProviderSettingConfig
  | ButtonSettingConfig
  | CustomSettingConfig

export interface SettingGroupSwitchConfig {
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  /** loading disables interaction */
  loading?: boolean
  ariaLabel?: string
  preview?: ToggleSwitchPreview
}

export interface SettingGroupConfig {
  title?: string
  /** default `sg-` from title; nested grid groups skip TOC */
  id?: string
  /** default true; top-level titled groups only */
  toc?: boolean
  icon?: ReactNode | string
  titleExtra?: ReactNode
  switch?: SettingGroupSwitchConfig
  detail?: ReactNode
  guide?: ReactNode
  /** preferred over title slug for search */
  guidePath?: string
  detailTone?: 'default' | 'warning' | 'info'
  description?: ReactNode
  /** default tooltip-only */
  descriptionVisible?: boolean
  items?: SettingItemConfig[]
  children?: ReactNode
  collapsible?: boolean
  defaultExpanded?: boolean
  className?: string
}

export interface SettingSectionConfig {
  /** icon tint key */
  sectionId?: string
  title: string
  icon?: ReactNode | string
  titleExtra?: ReactNode
  detail?: ReactNode
  guide?: ReactNode
  guidePath?: string
  detailTone?: 'default' | 'warning' | 'info'
  description?: string
  /** default tooltip-only */
  descriptionVisible?: boolean
  groups?: SettingGroupConfig[]
  children?: ReactNode
  className?: string
  animated?: boolean
}

export interface PermissionItem {
  key: string
  label: string
  code?: string
  hint?: string
}

export interface PermissionGroupConfig {
  title: string
  description?: string
  guide?: ReactNode
  guidePath?: string
  permissions: PermissionItem[]
  values: Record<string, boolean>
  onChange: (key: string, value: boolean) => void
  disabled?: boolean
  loading?: boolean
}

export interface QuotaItem {
  key: string
  label: string
  hint?: string
  min?: number
  max?: number
  step?: number
  unit?: string
}

export interface QuotaGroupConfig {
  title: string
  description?: string
  guide?: ReactNode
  guidePath?: string
  quotas: QuotaItem[]
  values: Record<string, number>
  onChange: (key: string, value: number) => void
  disabled?: boolean
  loading?: boolean
}

export interface InfoCardConfig {
  title?: string
  content: ReactNode
  variant?: 'default' | 'info' | 'warning' | 'success' | 'error'
  icon?: ReactNode | string
  className?: string
}
