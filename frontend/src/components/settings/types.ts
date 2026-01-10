/**
 * 通用设置组件类型定义
 */

import type { CSSProperties, ReactNode } from 'react'

// ==========================================
// 基础类型
// ==========================================

/** 设置项类型枚举 */
export type SettingType
  = | 'switch' // 开关
    | 'input' // 文本输入
    | 'number' // 数字输入
    | 'select' // 下拉选择
    | 'provider' // 服务商选择器（带图标的按钮组）
    | 'slider' // 滑动条
    | 'button' // 操作按钮
    | 'checkbox' // 复选框
    | 'custom' // 自定义渲染

/** 设置项尺寸 */
export type SettingSize = 'sm' | 'md' | 'lg'

/** 设置项布局 */
export type SettingLayout = 'horizontal' | 'vertical'

/** 选项类型（用于 select/provider） */
export interface SettingOption<T = string> {
  value: T
  label: string
  icon?: ReactNode | string
  badge?: string
  description?: string
  disabled?: boolean
}

// ==========================================
// 设置项配置
// ==========================================

/** 基础设置项配置 */
export interface BaseSettingItemConfig {
  /** 唯一标识（用于生成 id 和 name，注意：不要与 React 的 key 混淆） */
  itemKey?: string
  /** 显示标签 */
  label: string
  /** 描述说明（显示在标签下方） */
  description?: string
  /** 提示文本（显示在控件下方） */
  hint?: string
  /** 是否必填 */
  required?: boolean
  /** 是否禁用 */
  disabled?: boolean
  /** 加载状态 */
  loading?: boolean
  /** 错误信息 */
  error?: string
  /** 尺寸 */
  size?: SettingSize
  /** 布局方向 */
  layout?: SettingLayout
  /** 自定义样式 */
  style?: CSSProperties
  /** 自定义 class */
  className?: string
}

/** 开关设置项配置 */
export interface SwitchSettingConfig extends BaseSettingItemConfig {
  type: 'switch'
  value: boolean
  onChange: (value: boolean) => void
}

/** 复选框设置项配置 */
export interface CheckboxSettingConfig extends BaseSettingItemConfig {
  type: 'checkbox'
  value: boolean
  onChange: (value: boolean) => void
  /** 复选框标签（显示在复选框后面） */
  checkboxLabel?: string
}

/** 文本输入设置项配置 */
export interface InputSettingConfig extends BaseSettingItemConfig {
  type: 'input'
  value: string
  onChange: (value: string) => void
  onFocus?: () => void
  onBlur?: () => void
  /** 占位符 */
  placeholder?: string
  /** 输入类型 */
  inputType?: 'text' | 'password' | 'url' | 'email'
  /** 是否多行 */
  multiline?: boolean
  /** 多行时的行数 */
  rows?: number
  /** 自动完成 */
  autoComplete?: string
  /** 密码掩码自动选中 */
  autoSelectOnMask?: boolean
  /** 验证函数 */
  validate?: (value: string) => string | null
  /** 复制按钮 */
  copyable?: boolean
}

/** 数字输入设置项配置 */
export interface NumberSettingConfig extends BaseSettingItemConfig {
  type: 'number'
  value: number
  onChange: (value: number) => void
  min?: number
  max?: number
  step?: number
  /** 单位标签 */
  unit?: string
}

/** 下拉选择设置项配置 */
export interface SelectSettingConfig<T = string> extends BaseSettingItemConfig {
  type: 'select'
  value: T
  onChange: (value: T) => void
  options: SettingOption<T>[]
}

/** 服务商选择器设置项配置 */
export interface ProviderSettingConfig<T = string> extends BaseSettingItemConfig {
  type: 'provider'
  value: T
  onChange: (value: T) => void
  options: SettingOption<T>[]
}

/** 滑动条设置项配置 */
export interface SliderSettingConfig extends BaseSettingItemConfig {
  type: 'slider'
  value: number
  onChange: (value: number) => void
  min: number
  max: number
  step?: number
  /** 显示当前值 */
  showValue?: boolean
  /** 值格式化函数 */
  formatValue?: (value: number) => string
  /** 刻度标记 */
  marks?: Record<number, string>
}

/** 按钮设置项配置 */
export interface ButtonSettingConfig extends Omit<BaseSettingItemConfig, 'label'> {
  type: 'button'
  /** 可选标签（按钮可能不需要标签） */
  label?: string
  onClick: () => void
  /** 按钮文本 */
  buttonText: string
  /** 按钮图标 */
  buttonIcon?: ReactNode | string
  /** 按钮变体 */
  variant?: 'primary' | 'secondary' | 'danger'
}

/** 自定义设置项配置 */
export interface CustomSettingConfig extends BaseSettingItemConfig {
  type: 'custom'
  /** 自定义渲染函数 */
  render: () => ReactNode
}

/** 设置项配置联合类型 */
export type SettingItemConfig
  = | SwitchSettingConfig
    | CheckboxSettingConfig
    | InputSettingConfig
    | NumberSettingConfig
    | SelectSettingConfig
    | ProviderSettingConfig
    | SliderSettingConfig
    | ButtonSettingConfig
    | CustomSettingConfig

// ==========================================
// 分组与区块
// ==========================================

/** 设置组配置 */
export interface SettingGroupConfig {
  /** 组标题 */
  title?: string
  /** 组图标 */
  icon?: ReactNode | string
  /** 组描述 */
  description?: string
  /** 子项 */
  items?: SettingItemConfig[]
  /** 子元素 */
  children?: ReactNode
  /** 是否可折叠 */
  collapsible?: boolean
  /** 默认是否展开 */
  defaultExpanded?: boolean
  /** 自定义样式 */
  className?: string
}

/** 设置区块配置 */
export interface SettingSectionConfig {
  /** 区块标题 */
  title: string
  /** 区块图标 */
  icon?: ReactNode | string
  /** 区块描述 */
  description?: string
  /** 子组 */
  groups?: SettingGroupConfig[]
  /** 子元素 */
  children?: ReactNode
  /** 自定义样式 */
  className?: string
  /** 动画配置 */
  animated?: boolean
}

// ==========================================
// 权限与配额预设
// ==========================================

/** 权限项配置 */
export interface PermissionItem {
  key: string
  label: string
  code?: string
  hint?: string
}

/** 权限组配置 */
export interface PermissionGroupConfig {
  title: string
  description?: string
  permissions: PermissionItem[]
  values: Record<string, boolean>
  onChange: (key: string, value: boolean) => void
  disabled?: boolean
  loading?: boolean
}

/** 配额项配置 */
export interface QuotaItem {
  key: string
  label: string
  hint?: string
  min?: number
  max?: number
  step?: number
  unit?: string
}

/** 配额组配置 */
export interface QuotaGroupConfig {
  title: string
  description?: string
  quotas: QuotaItem[]
  values: Record<string, number>
  onChange: (key: string, value: number) => void
  disabled?: boolean
  loading?: boolean
}

// ==========================================
// 信息卡片
// ==========================================

/** 信息卡片配置 */
export interface InfoCardConfig {
  title?: string
  content: ReactNode
  variant?: 'default' | 'info' | 'warning' | 'success' | 'error'
  icon?: ReactNode | string
  className?: string
}

// ==========================================
// 工具类型
// ==========================================

/** 根据类型获取设置项配置 */
export type SettingConfigByType<T extends SettingType>
  = T extends 'switch' ? SwitchSettingConfig
    : T extends 'checkbox' ? CheckboxSettingConfig
      : T extends 'input' ? InputSettingConfig
        : T extends 'number' ? NumberSettingConfig
          : T extends 'select' ? SelectSettingConfig
            : T extends 'provider' ? ProviderSettingConfig
              : T extends 'slider' ? SliderSettingConfig
                : T extends 'button' ? ButtonSettingConfig
                  : T extends 'custom' ? CustomSettingConfig
                    : never

/** 设置值类型映射 */
export type SettingValueType<T extends SettingType>
  = T extends 'switch' | 'checkbox' ? boolean
    : T extends 'input' ? string
      : T extends 'number' | 'slider' ? number
        : T extends 'select' | 'provider' ? string
          : T extends 'button' | 'custom' ? never
            : unknown
