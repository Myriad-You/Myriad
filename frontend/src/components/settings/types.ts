/**
 * 通用设置组件类型定义
 */

import type { CSSProperties, ReactNode } from 'react'

// 基础类型

/** 设置项类型枚举 */
export type SettingType =
  | 'switch' // 开关
  | 'input' // 文本输入
  | 'number' // 数字输入
  | 'slider' // 滑动条
  | 'select' // 下拉选择
  | 'provider' // 服务商选择器（带图标的按钮组）
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

// 设置项配置

/** 基础设置项配置 */
export interface BaseSettingItemConfig {
  /** 唯一标识（用于生成 id 和 name，注意：不要与 React 的 key 混淆） */
  itemKey?: string
  /** 显示标签 */
  label: string
  /**
   * 详细说明：默认不展示，标签旁 ⓘ hover 显示 tooltip。
   * 适合较长帮助文案；短说明仍可用 description。
   */
  detail?: ReactNode
  /**
   * 选项详细指南。
   * 「显示说明」开启时标题旁出现入口；点击后以浮窗展示（优先上方，不够则左侧）。
   * 一句话总结请用 description；中等说明用 detail；长文用 guide。
   */
  guide?: ReactNode
  /**
   * 指南目录路径（如 advanced.proxyEnable）。
   * 写入 DOM 锚点，供配置搜索「点指南 → 滚到选项」。
   * 推荐用 useSettingGuide().bindGuide(path, entry) 一并传入。
   */
  guidePath?: string
  /** 描述说明（显示在标签下方） */
  description?: string
  /** 提示文本（显示在控件下方） */
  hint?: string
  /** 是否必填 */
  required?: boolean
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

/** 文本输入展示变体 */
export type InputItemVariant = 'default' | 'clickToEdit' | 'imageUpload'

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
  inputType?: 'text' | 'password' | 'url' | 'email' | 'search'
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
  /**
   * - default：常规可编辑输入
   * - clickToEdit：只读展示，点击后编辑；保存走 onCommit（不依赖 onChange 草稿）
   * - imageUpload：URL 输入 + 本地图片上传（data URL）+ 预览
   */
  variant?: InputItemVariant
  /** clickToEdit：空值展示文案 */
  emptyLabel?: string
  /** clickToEdit：展示态操作文案 */
  editLabel?: string
  /** clickToEdit：保存按钮文案 */
  saveLabel?: string
  /** clickToEdit：取消 aria / title */
  cancelLabel?: string
  /**
   * clickToEdit：提交草稿。返回 Promise 时展示 loading；
   * resolve 后退出编辑（reject / 抛错则保持编辑态）。
   */
  onCommit?: (next: string) => void | Promise<void>
  /** clickToEdit：进入编辑态 */
  onEditStart?: () => void
  /** clickToEdit：取消编辑 */
  onEditCancel?: () => void
  /**
   * imageUpload：file input accept，默认 `image/*`
   * （如 `image/png,image/jpeg,image/webp,image/gif,image/svg+xml`）
   */
  accept?: string
  /** imageUpload：最大文件字节数，默认 512KB */
  maxImageBytes?: number
  /** imageUpload：上传按钮文案 */
  uploadLabel?: string
  /** imageUpload：清除按钮 aria / title */
  clearImageLabel?: string
  /** imageUpload：本地 data URL 时输入区展示文案 */
  localImageLabel?: string
  /** imageUpload：预览 alt */
  previewAlt?: string
  /** imageUpload：是否显示清除按钮（有值时），默认 true */
  clearable?: boolean
  /** imageUpload：非图片类型错误文案 */
  imageTypeError?: string
  /** imageUpload：超限错误文案 */
  imageSizeError?: string
  /** imageUpload：读取失败错误文案 */
  imageReadError?: string
}

/** 数字输入设置项配置 */
export interface NumberSettingConfig extends BaseSettingItemConfig {
  type: 'number'
  value: number
  onChange: (value: number) => void
  onBlur?: () => void
  min?: number
  max?: number
  step?: number
  /** 单位标签 */
  unit?: string
}

/** 滑动条设置项配置 */
export interface SliderSettingConfig extends BaseSettingItemConfig {
  type: 'slider'
  value: number
  onChange: (value: number) => void
  onBlur?: () => void
  min?: number
  max?: number
  step?: number
  /** 单位标签（跟在数值后，如 `px`） */
  unit?: string
  /** 是否显示当前数值，默认 true */
  showValue?: boolean
  /** 自定义数值展示 */
  formatValue?: (value: number) => string
  /**
   * 轨道下方显示两端。默认 false。
   * 开启后两端始终显示对应数值（min / max）；
   * 可另传 `startLabel` / `endLabel` 作为数值旁的说明文案。
   */
  showRangeLabels?: boolean
  /**
   * 左端（min）说明文案，如「弱 · 更清晰」。
   * 有值时自动显示两端行；数值始终显示。
   */
  startLabel?: string
  /**
   * 右端（max）说明文案，如「强 · 更模糊」。
   * 有值时自动显示两端行；数值始终显示。
   */
  endLabel?: string
  /**
   * 推荐值（与 value 同单位）。在进度条下方对应位置显示标记。
   * 超出 min/max 时不显示。
   */
  recommendedValue?: number
  /** 推荐标记文案，默认「推荐」类短词；可含数值说明 */
  recommendedLabel?: string
}

/** 下拉选择设置项配置 */
export interface SelectSettingConfig<T = string> extends BaseSettingItemConfig {
  type: 'select'
  value: T
  onChange: (value: T) => void
  options: SettingOption<T>[]
}

/** 服务商选择器设置项配置 */
export interface ProviderSettingConfig<
  T = string,
> extends BaseSettingItemConfig {
  type: 'provider'
  value: T
  onChange: (value: T) => void
  options: SettingOption<T>[]
}

/** 按钮设置项配置 */
export interface ButtonSettingConfig extends Omit<
  BaseSettingItemConfig,
  'label'
> {
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

// 分组与区块

/** 子分类标题行右侧开关（模块启用等） */
export interface SettingGroupSwitchConfig {
  /** 当前是否开启 */
  checked: boolean
  /** 开关变化 */
  onChange: (checked: boolean) => void
  disabled?: boolean
  /** 加载中（等同禁用交互） */
  loading?: boolean
  /** 无障碍名；默认使用组 title */
  ariaLabel?: string
}

/** 设置组配置 */
export interface SettingGroupConfig {
  /** 组标题 */
  title?: string
  /**
   * 锚点 id（页内快速跳转）。缺省时由 title 生成 `sg-…`。
   * 网格内嵌套 Group 不会进入 TOC。
   */
  id?: string
  /**
   * 是否进入页内 TOC。默认 true；设 false 可隐藏。
   * 仅顶层（非 SettingGroupGrid 内）且有 title 时生效。
   */
  toc?: boolean
  /** 组图标 */
  icon?: ReactNode | string
  /** 标题旁附加内容（如 SettingTitleTag 跳转标签） */
  titleExtra?: ReactNode
  /**
   * 标题行右侧开关。
   * 用于「模块总开关」等：标题左侧信息，右侧 ToggleSwitch。
   */
  switch?: SettingGroupSwitchConfig
  /**
   * 详细说明（推荐）：默认不展示，标题旁 ⓘ hover/focus 显示 tooltip。
   * 与 description 同时存在时优先使用 detail。
   */
  detail?: ReactNode
  /**
   * 分组详细指南。「显示说明」开启时标题旁入口；点击后浮窗展示。
   */
  guide?: ReactNode
  /**
   * 指南目录路径（如 advanced.network）。
   * 优先于 title 生成稳定锚点 id，供搜索跳转。
   */
  guidePath?: string
  /** tooltip 语气：warning 用于阻断性提示 */
  detailTone?: 'default' | 'warning' | 'info'
  /**
   * 组说明。默认作为 tooltip（与 detail 相同交互）；
   * 设 descriptionVisible 时额外常显在标题下方。
   * 用作一句话总结；长文请用 guide。
   */
  description?: ReactNode
  /**
   * 是否在标题下常显 description。默认 false（仅 tooltip）。
   */
  descriptionVisible?: boolean
  /** 子项 */
  items?: SettingItemConfig[]
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
  /** 区块标识（用于图标背景色区分） */
  sectionId?: string
  /** 区块标题 */
  title: string
  /** 区块图标 */
  icon?: ReactNode | string
  /** 标题旁附加（如 SettingTitleTag 跳转 / 操作 chip） */
  titleExtra?: ReactNode
  /**
   * 详细说明：默认不展示，标题旁 ⓘ hover 显示 tooltip。
   * 优先于 description 作为 tooltip 内容。
   */
  detail?: ReactNode
  /**
   * 区块详细指南。「显示说明」开启时标题旁入口；点击后浮窗展示。
   */
  guide?: ReactNode
  /** 指南路径，供搜索跳转锚点 */
  guidePath?: string
  /** tooltip 语气 */
  detailTone?: 'default' | 'warning' | 'info'
  /**
   * 区块说明。默认作为 tooltip；
   * descriptionVisible 时额外常显。
   */
  description?: string
  /** 是否常显 description。默认 false */
  descriptionVisible?: boolean
  /** 子组 */
  groups?: SettingGroupConfig[]
  children?: ReactNode
  /** 自定义样式 */
  className?: string
  /** 动画配置 */
  animated?: boolean
}

// 权限与配额预设

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
  /** 选项指南（标题旁 ⓘ / 显示说明入口） */
  guide?: ReactNode
  /** 指南路径，供搜索跳转锚点 */
  guidePath?: string
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
  /** 选项指南（标题旁 ⓘ / 显示说明入口） */
  guide?: ReactNode
  /** 指南路径，供搜索跳转锚点 */
  guidePath?: string
  quotas: QuotaItem[]
  values: Record<string, number>
  onChange: (key: string, value: number) => void
  disabled?: boolean
  loading?: boolean
}

// 信息卡片

/** 信息卡片配置 */
export interface InfoCardConfig {
  title?: string
  content: ReactNode
  variant?: 'default' | 'info' | 'warning' | 'success' | 'error'
  icon?: ReactNode | string
  className?: string
}

// 工具类型
