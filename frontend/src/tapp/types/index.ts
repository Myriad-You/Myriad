/**
 * Tapp 系统类型定义
 * 核心类型声明文件
 */

// ============ 基础类型 ============

/** 权限等级 */
export type PermissionLevel = 'public' | 'basic' | 'elevated' | 'privileged'

/** Tapp 状态 */
export type TappStatus = 'installed' | 'running' | 'suspended' | 'error'

/** 小组件尺寸（与系统保持一致） */
export type WidgetSize
  = | '1x1' | '1x2' | '2x1' | '2x2'
    | '2x3' | '3x2' | '4x2' | '2x4'
    | '3x3' | '4x4'

/** 小组件分类 */
export type WidgetCategory = 'stats' | 'activity' | 'visualization' | 'utility' | 'custom'

/** 平台数据类型 */
export type PlatformDataType = 'game' | 'video' | 'music' | 'anime' | 'article' | 'custom'

// ============ Tapp Manifest ============

/** Tapp 清单文件 */
export interface TappManifest {
  /** 唯一标识符 (如 com.example.my-tapp) */
  id: string

  /** 显示名称 */
  name: string

  /** 版本号 (semver) */
  version: string

  /** 描述 */
  description: string

  /** 作者信息 */
  author?: {
    name: string
    email?: string
    url?: string
  }

  /** 图标（emoji 或 URL） */
  icon?: string

  /** 内联 SVG 图标代码（优先于 icon 字段） */
  iconSvg?: string

  /** 主题色（十六进制，如 #6366f1） */
  themeColor?: string

  /** 入口文件 */
  main: string

  /** 所需权限 */
  permissions: TappPermission[]

  /** 可选权限（运行时请求） */
  optionalPermissions?: TappPermission[]

  /** 内容安全策略 */
  contentSecurityPolicy?: {
    'connect-src'?: string
    'script-src'?: string
    'style-src'?: string
    'img-src'?: string
  }

  /** 最低系统版本要求 */
  minSystemVersion?: string

  /** 主页 URL */
  homepage?: string

  /** 仓库 URL */
  repository?: string

  /** 小组件定义（声明式，安装时自动注册） */
  widgets?: ManifestWidget[]

  /** 是否有页面模块（声明式，标识应用可在页面模式下运行） */
  hasPage?: boolean

  /**
   * CSS 架构模式
   * - 'unified': 统一 CSS 文件（默认，使用 styles 字段）
   * - 'separated': 分离 CSS 文件（使用 widgetStyles + pageStyles）
   */
  cssMode?: 'unified' | 'separated'

  /** 自定义 CSS 样式文件路径（统一模式，或作为共享样式） */
  styles?: string

  /** Widget 专用 CSS 文件路径（分离模式） */
  widgetStyles?: string

  /** Page 专用 CSS 文件路径（分离模式） */
  pageStyles?: string

  /** 页面 HTML 模板文件路径 */
  pageTemplate?: string

  /** 设置项定义 */
  settings?: TappSettingItem[]
}

/** Manifest 中的 Widget 声明 */
export interface ManifestWidget {
  /** 组件 ID（Tapp 内唯一） */
  id: string
  /** 显示名称 */
  name: string
  /** 描述 */
  description?: string
  /** 图标（emoji 或 icon 名称） */
  icon?: string
  /** 默认尺寸 */
  defaultSize: WidgetSize
  /** 支持的尺寸 */
  sizes: WidgetSize[]
  /** 组件分类 */
  category?: WidgetCategory
  /** 刷新间隔（毫秒） */
  refreshInterval?: number
  /** 配置 Schema */
  configSchema?: WidgetConfigSchema
  /** HTML 模板文件路径（按尺寸） */
  templates?: Record<string, string>
}

/** Tapp 设置项类型 */
export type TappSettingType = 'toggle' | 'select' | 'input' | 'number' | 'color'

/** Tapp 设置项定义 */
export interface TappSettingItem {
  /** 设置项 key（用于 storage） */
  key: string
  /** 显示名称 */
  label: string
  /** 设置项类型 */
  type: TappSettingType
  /** 描述 */
  description?: string
  /** 默认值 */
  defaultValue?: unknown
  /** select 类型的选项 */
  options?: { value: string, label: string }[]
  /** number 类型的范围 */
  min?: number
  max?: number
  step?: number
  /** input 类型的 placeholder */
  placeholder?: string
}

/** 权限类型 */
export type TappPermission
  // 小组件权限
  = | 'widget:register'
  // 平台数据权限
    | 'platform:read'
    | 'platform:write'
    | 'platform:register'
  // AI 权限
    | 'ai:generate'
    | 'ai:analyze'
    | 'ai:chat'
    | 'ai:image'
  // 报告权限
    | 'report:read'
    | 'report:write'
  // 存储权限
    | 'storage'
  // UI 权限
    | 'ui:notification'
    | 'ui:fullscreen'
    | 'ui:theme'
    | 'ui:confirm'
  // P0: 网络权限
    | 'network:fetch'
  // P1: 媒体权限
    | 'media:control'
    | 'media:read'
  // P2: 组件注册权限
    | 'component:theme'
    | 'component:agent'
  // P2: 快捷键权限
    | 'shortcut:register'
  // P2: 事件权限
    | 'event:publish'
    | 'event:subscribe'
  // P3: 定时任务权限
    | 'scheduler:register'

// ============ 用户角色 ============

/** 用户角色类型 */
export type UserRole = 'guest' | 'user' | 'admin'

// ============ 后台运行需求 ============

/** 后台运行需求类型 */
export type BackgroundRequirement
  = | 'widget' // 有小组件在主页显示
    | 'media' // 媒体控制（如音乐播放器扩展）
    | 'sync' // 后台数据同步
    | 'notification' // 定时通知
    | 'scheduler' // 定时任务
    | 'event-listener' // 事件监听（跨 Tapp 通信）
    | 'realtime' // 实时数据更新

/** 后台运行需求声明 */
export interface BackgroundRequirementDeclaration {
  /** 需求类型 */
  type: BackgroundRequirement
  /** 需求描述（用于显示给用户） */
  reason?: string
  /** 是否为必需（false 表示可选，用户可关闭） */
  required?: boolean
}

// ============ Tapp 实例 ============

/** Tapp 实例信息 */
export interface TappInstance {
  /** 实例 ID */
  id: string

  /** 清单信息 */
  manifest: TappManifest

  /** 当前状态 */
  status: TappStatus

  /** 安装时间 */
  installedAt: string

  /** 最后运行时间 */
  lastRunAt?: string

  /** 已授权的权限 */
  grantedPermissions: TappPermission[]

  /**
   * 当前用户角色
   * - guest: 未登录用户（只能查看管理员的 Tapp）
   * - user: 普通用户（只能使用 basic 权限）
   * - admin: 管理员（可使用所有权限）
   */
  userRole: UserRole

  /** 是否为临时安装（普通用户安装的 Tapp，退出登录后移除） */
  isTemporary?: boolean

  /** 是否为管理员的 Tapp（对所有用户可见） */
  isAdminTapp?: boolean

  /** 配额使用情况 */
  quotaUsage?: {
    ai: {
      dailyCalls: number
      dailyTokens: number
      lastReset: string
    }
    storage: {
      used: number
      limit: number
    }
  }

  /** 错误信息（如果状态为 error） */
  error?: string
}

// ============ 小组件注册 ============

/** 小组件注册配置 */
export interface WidgetRegistration {
  /** 组件 ID（Tapp 内唯一） */
  id: string

  /** 显示名称 */
  name: string

  /** 描述 */
  description: string

  /** 图标 */
  icon: string

  /** 支持的尺寸 */
  sizes: WidgetSize[]

  /** 默认尺寸 */
  defaultSize: WidgetSize

  /** 组件分类 */
  category: WidgetCategory

  /** 配置 Schema */
  configSchema?: WidgetConfigSchema

  /** 刷新间隔（毫秒，最小 60000） */
  refreshInterval?: number
}

/** 小组件配置 Schema */
export interface WidgetConfigSchema {
  type: 'object'
  properties: Record<string, {
    type: 'string' | 'number' | 'boolean' | 'select'
    title: string
    description?: string
    default?: unknown
    options?: Array<{ label: string, value: unknown }>
  }>
  required?: string[]
}

/** 已注册的小组件 */
export interface RegisteredWidget {
  /** 完整 ID: tapp.{tappId}.{widgetId} */
  id: string

  /** 所属 Tapp ID */
  tappId: string

  /** 组件配置 */
  config: WidgetRegistration

  /** 实例数量 */
  instanceCount: number

  /** 注册时间 */
  registeredAt: string
}

/** 小组件渲染属性 */
export interface WidgetRenderProps {
  /** 当前尺寸 */
  size: WidgetSize

  /** 用户配置 */
  config: Record<string, unknown>

  /** 是否编辑模式 */
  isEditMode: boolean

  /** 是否预览模式 */
  isPreview: boolean

  /** 缩放比例 */
  scale: number

  /** 字体缩放 */
  fontScale: number

  /** 主题 */
  theme: 'light' | 'dark'

  /** 主题色 */
  primaryColor?: string

  /** 语言 */
  locale?: string
}

// ============ 平台数据 ============

/** 平台信息 */
export interface PlatformInfo {
  id: string
  name: string
  icon: string
  color: string
  enabled: boolean
  isTappPlatform: boolean
  tappId?: string
}

/** 新增平台数据条目 */
export interface NewPlatformItem {
  /** 目标平台 */
  platform: string

  /** 数据类型 */
  type: PlatformDataType

  /** 标题（必填） */
  title: string

  /** 封面图 */
  cover?: string

  /** 描述 */
  description?: string

  /** 原始链接 */
  url?: string

  /** 自定义元数据 */
  metadata?: Record<string, unknown>

  /** 创建时间 */
  createdAt?: string
}

/** 平台数据条目结果 */
export interface PlatformItemResult {
  success: boolean
  itemId: string
  source: string
}

/** 自定义平台配置 */
export interface CustomPlatformConfig {
  /** 平台 ID（Tapp 内唯一） */
  id: string

  /** 显示名称 */
  name: string

  /** 图标 */
  icon: string

  /** 主题色 */
  color: string

  /** 描述 */
  description: string

  /** 支持的数据类型 */
  supportedTypes: PlatformDataType[]

  /** URL 模式 */
  urlPattern?: string
}

// ============ AI 相关 ============

/** AI 生成请求 */
export interface AIGenerateRequest {
  /** 提示词（最大 2000 字符） */
  prompt: string

  /** 上下文配置 */
  context?: {
    includePlatformStats?: boolean
    includeReportSummary?: boolean
    customData?: Record<string, unknown>
  }

  /** 生成选项 */
  options?: {
    maxTokens?: number
    temperature?: number
    format?: 'text' | 'json'
  }
}

/** AI 生成响应 */
export interface AIGenerateResponse {
  success: boolean
  result: string
  usage: {
    promptTokens: number
    completionTokens: number
    totalTokens: number
  }
  quotaRemaining: number
}

/** AI 分析请求 */
export interface AIAnalyzeRequest {
  /** 要分析的数据 */
  data: unknown

  /** 分析类型 */
  type: 'summarize' | 'categorize' | 'sentiment' | 'custom'

  /** 自定义指令 */
  instruction?: string
}

/** AI 分析响应 */
export interface AIAnalyzeResponse {
  success: boolean
  analysis: unknown
  confidence?: number
  quotaRemaining: number
}

/** AI 配额状态（基于用户角色的限额） */
export interface AIQuotaStatus {
  /** 每日调用次数限制 */
  daily: {
    limit: number
    used: number
    resetsAt: string
  }
  /** Token 限制 */
  tokens: {
    limit: number
    used: number
    resetsAt: string
  }
  /** 冷却时间 */
  cooldown: {
    required: number
    remaining: number
  }
  /** 是否被限制 */
  restricted: boolean
  /** 限制原因 */
  restrictionReason?: string
  /** 是否无限制（管理员） */
  unlimited?: boolean
  /** 当前用户角色 */
  userRole?: UserRole
}

/** AI 限额配置（从后端获取） */
export interface AIQuotaLimits {
  /** 每日调用次数限制 */
  dailyCalls: number
  /** 每日 Token 限制 */
  dailyTokens: number
  /** 冷却时间（秒） */
  cooldownSeconds: number
  /** 是否无限制 */
  unlimited: boolean
}

// ============ 消息通信 ============

/** Bridge 消息类型 */
export type TappMessageType = 'request' | 'response' | 'event'

/** Bridge 消息结构 */
export interface TappMessage<T = unknown> {
  /** 消息类型 */
  type: TappMessageType

  /** 消息 ID */
  id: string

  /** 操作名称 */
  action: string

  /** 数据载荷 */
  payload: T

  /** 来源 Tapp ID */
  source?: string

  /** 时间戳 */
  timestamp: number

  /** 错误信息 */
  error?: string
}

/** API 调用请求 */
export interface TappAPIRequest {
  /** API 路径 (如 widget.register) */
  api: string

  /** 方法名 */
  method: string

  /** 参数 */
  args: unknown[]
}

/** API 调用响应 */
export interface TappAPIResponse<T = unknown> {
  success: boolean
  data?: T
  error?: string
  code?: string
}

// ============ 配额管理 ============

/** 配额配置 */
export interface TappQuotaConfig {
  ai: {
    dailyLimit: number
    monthlyLimit: number
    maxTokensPerRequest: number
  }
  platform: {
    readPerMinute: number
    writePerMinute: number
    maxItemsPerBatch: number
  }
  storage: {
    maxKeys: number
    maxValueSize: number
    maxTotalSize: number
  }
  widget: {
    maxRegistrations: number
    minRefreshInterval: number
  }
}

/** 使用统计 */
export interface TappUsageStats {
  tappId: string
  ai: {
    used: number
    limit: number
    remaining: number
    resetAt: string
  }
  platformRead: {
    used: number
    limit: number
    remaining: number
    resetAt: string
  }
  platformWrite: {
    used: number
    limit: number
    remaining: number
    resetAt: string
  }
  history: {
    type: string
    count: number
    lastReset: string
  }[]
}
