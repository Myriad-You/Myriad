/**
 * Tapp 系统类型定义
 * 核心类型声明文件
 */

// 基础类型

/** 权限等级 */
export type PermissionLevel = 'public' | 'basic' | 'elevated' | 'privileged'

/** Tapp 状态 */
export type TappStatus = 'installed' | 'running' | 'suspended' | 'error'

/** 小组件尺寸（与系统保持一致） */
export type WidgetSize =
  | '1x1'
  | '1x2'
  | '2x1'
  | '2x2'
  | '2x3'
  | '3x2'
  | '4x1'
  | '4x2'
  | '2x4'
  | '3x3'
  | '4x4'

/** 小组件分类 */
export type WidgetCategory =
  'stats' | 'activity' | 'visualization' | 'utility' | 'custom'

/** 平台数据类型 */
export type PlatformDataType =
  'game' | 'video' | 'music' | 'anime' | 'article' | 'custom'

/** Tapp 用途分类；Page / Widget 等运行形态不属于此字段。 */
export type TappCategory =
  | 'ai'
  | 'data'
  | 'developer'
  | 'game'
  | 'media'
  | 'productivity'
  | 'social'
  | 'utility'

// Tapp Manifest

/** 单个语言下的清单展示文案覆盖 */
export interface TappManifestLocaleEntry {
  /** 该语言下的显示名称 */
  name?: string
  /** 该语言下的描述 */
  description?: string
}

/**
 * 清单展示文案的多语言覆盖。
 * 键为 BCP-47 语言标签（如 zh-CN、en-US、ja-JP）；
 * 顶层 name/description 作为所有语言未命中时的回退值。
 */
export type TappManifestLocales = Record<string, TappManifestLocaleEntry>

/** 共享层：所有模式先执行的入口，也是 headless 后台唯一执行的代码。 */
export interface TappCoreLayer {
  /** 包内相对路径的 `.js` 入口；层内其余文件由它 require 进来。 */
  entry: string
  /** 各层共享的作者样式。宿主预编译产物不走这里。 */
  styles?: string
}

/** Page 层。`entry` 与 `template` 至少有一个。 */
export interface TappPageLayer {
  entry?: string
  template?: string
  styles?: string
}

/** Tapp 清单文件 */
export interface TappManifest {
  /** 唯一标识符 (如 com.example.my-tapp) */
  id: string

  /** 显示名称 */
  name: string

  /** 版本号 (semver) */
  version: string

  description?: string

  /**
   * name/description 的多语言覆盖；按宿主当前语言解析，
   * 未命中回退到顶层 name/description。
   */
  locales?: TappManifestLocales

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

  /**
   * 可选：自定义全彩图 / 全彩 SVG 仍套 material 色壳（不铺满 standalone）。
   * 默认 auto：bitmap / 非 currentColor SVG → standalone；currentColor SVG / emoji → 色壳。
   */
  iconShell?: boolean

  /** 主题色（十六进制，如 #6366f1） */
  themeColor?: string

  /** 所需权限 */
  permissions: TappPermission[]

  /** 主页 URL */
  homepage?: string

  /** 仓库 URL */
  repository?: string

  /** 最低兼容 Myriad 版本（语义版本） */
  minSystemVersion?: string

  /** 小组件定义（声明式，安装时自动注册） */
  widgets?: ManifestWidget[]

  /**
   * 共享层。所有沙箱模式先执行它，headless 后台只执行它。
   * 声明 `backgroundRequirements` 的应用必须有 core，否则没有可常驻的代码。
   */
  core?: TappCoreLayer

  /** Page 层。声明即表示该应用有可打开的页面（取代旧的 hasPage 开关）。 */
  page?: TappPageLayer

  /**
   * 声明式后台运行需求（启动时自动注册，用于引导 headless core）。
   * 声明需求的 Tapp 会在运行期由 TappBackgroundRunner 拉起无头 core 沙箱，
   * 即使没有可见窗口/widget 也持续运行 core 逻辑。
   */
  backgroundRequirements?: BackgroundRequirement[]

  /** Host-injected runtime libraries. Currently only `three`. */
  runtimeModules?: Array<'three'>

  /**
   * Turn-based game session on federation rooms.
   * Requires `game:session` plus federation room permissions.
   */
  game?: {
    protocol: string
    maxPlayers?: number
    maxMessageBytes?: number
  }

  /** 应用用途分类（稳定 ID，由宿主翻译显示） */
  category: TappCategory

  /** 设置项定义 */
  settings?: TappSettingItem[]

  /** 安装级只写凭据；值仅由宿主绑定到声明式 HTTP API。 */
  credentials?: TappCredentialItem[]

  /** 命名 API 声明；由后端执行并统一实施权限、缓存与出站访问控制 */
  apis?: Record<string, TappApiDefinition>

  /** 跨 Tapp 数据契约；实际读取仍需每次通过宿主授权弹窗。 */
  dataExchange?: TappDataExchangeManifest

  /** 服务端治理的 AI Task 能力声明。 */
  ai?: TappAIManifest

  /** 在线、at-most-once 的 Event Broker topic 声明。 */
  events?: TappEventsManifest

  /** Agent Interaction 声明。 */
  agent?: TappAgentManifest

  /**
   * 包内静态资源路径列表（相对安装根，必须位于 `assets/` 下）。
   * 二进制文件允许；运行时通过 `Tapp.assets` 读取，不走 `Tapp.storage`。
   */
  assets?: string[]

  /**
   * Host-mediated external links for `Tapp.ui.openUrl`.
   * Runtime only opens by declared `id` (+ optional path/query under match rules);
   * undeclared / free-form URLs are rejected.
   * Requires permission `ui:openUrl`.
   */
  openUrls?: TappOpenUrlDef[]
}

/** One install-time declared external navigation target. */
export interface TappOpenUrlDef {
  /** Stable id for `Tapp.ui.openUrl({ id })`. */
  id: string
  /** Base HTTPS URL (http only for localhost / 127.0.0.1). */
  url: string
  /**
   * How path/query may extend `url`:
   * - `exact` (default): only the declared URL
   * - `prefix`: same origin, path under the declared path prefix
   * - `origin`: any path/query on the declared origin
   */
  match?: 'exact' | 'prefix' | 'origin'
}

export type TappAIOperation = 'generate' | 'analyze' | 'chat' | 'image'
export type TappAIContextSource = 'platform' | 'report' | 'profile' | 'custom'
export type TappAIOutputFormat = 'text' | 'json' | 'image'

export interface TappAIManifest {
  protocolVersion: 2
  operations: TappAIOperation[]
  modelTier: 'standard' | 'pro'
  contextSources: TappAIContextSource[]
  outputFormats: TappAIOutputFormat[]
}

export interface TappEventsManifest {
  publish?: string[]
  subscribe?: string[]
}

export interface TappAgentManifest {
  protocolVersion: 2
  interactions: Array<{
    type: string
    inputSchema?: string
    resultSchema?: string
  }>
  intents?: Array<'ui.open' | 'report.create' | 'dataExchange.request'>
}

export type AgentInteractionState =
  'pending' | 'accepted' | 'completed' | 'rejected' | 'expired' | 'cancelled'

export interface AgentInteractionV2<TInput = unknown> {
  version: 2
  interactionId: string
  type: string
  tappId: string
  state: AgentInteractionState
  input: TInput
  inputSchema?: string
  resultSchema?: string
  deadline: string
  source: { agentId: string; taskId?: string }
  createdAt: string
  updatedAt: string
  result?: unknown
  rejectionReason?: string
}

export interface TappEvent<T = unknown> {
  version: 2
  eventId: string
  topic: string
  scope: 'instance' | 'owner'
  source: { tappId: string; runtimeId: string }
  payload: T
  occurredAt: string
  dedupeKey?: string
}

export interface PublishEventRequest {
  topic: string
  scope: 'instance' | 'owner'
  payload?: unknown
  dedupeKey?: string
}

export interface TappDataExchangeManifest {
  exports?: TappDataExport[]
  imports?: TappDataImport[]
}

export interface TappDataExport {
  id: string
  /** 受支持的内联 JSON Schema 子集；不支持 `$ref`。 */
  schema: Record<string, unknown>
  maxBytes: number
  maxRecords?: number
  description?: string
}

export interface TappDataImport {
  tappId: string
  exportId: string
}

/** Manifest 中的命名 API 声明 */
export interface TappApiDefinition {
  /**
   * 调用者范围：`public` 允许游客；`protected`（默认）要求登录主体；
   * `manager` 仅允许安装 owner 或当前管理员。
   * 与 `network:fetch` 无关——所有 `type: http` 声明 API 仍必须获得 `network:fetch`。
   */
  access?: 'public' | 'protected' | 'manager'
  /** HTTP 代理或平台内置能力 */
  type?: 'http' | 'builtin'
  /** HTTP 端点；支持后端模板变量 */
  endpoint?: string
  /** HTTP 方法，默认 GET */
  method?: string
  headers?: Record<string, string>
  /** 宿主凭据绑定；密钥不会进入模板上下文。 */
  credential?: {
    key: string
    /** 省略且声明 `header` 时视为 header（旧清单）。 */
    in?: 'header' | 'query' | 'form' | 'sign'
    field?: string
    header?: string
    prefix?: string
    encoding?: 'base64'
    sign?: {
      alg: 'md5-sorted-kv' | 'hmac-sha256-raw'
      over: string[]
      timestampField?: string
    }
  }
  /** 请求体序列化模式，默认 json */
  bodyMode?: 'json' | 'raw' | 'form'
  body?: unknown
  /** type=builtin 时的能力名，例如 geo、ai:chat、ai:generate */
  builtin?: string
  /** 后端上下文注入映射 */
  inject?: Record<string, string>
  /** 响应缓存秒数，0 表示不缓存 */
  cacheTtl?: number
  /** 区域伪装配置 */
  spoof?: string
  description?: string
}

export interface TappCredentialItem {
  key: string
  label: string
  description?: string
  placeholder?: string
}

/**
 * 宿主加载后的 Tapp 代码结构。
 * 这是运行时契约，不属于示例应用专用类型。
 */
export interface TappCodeStructure {
  /** 包内 `.js` 文件：相对路径 → 源码。只含当前模式依赖图内的文件。 */
  modules: Record<string, string>
  /** 宿主预解析的 require 图；旧后端响应可省略并由前端兼容扫描。 */
  moduleResolutions?: Record<string, Record<string, string>>
  /** core 层入口；headless 后台只执行它。 */
  coreEntry?: string
  /** Page 层入口。 */
  pageEntry?: string
  /** 各 widget 的入口：widget id → 相对路径。 */
  widgetEntries?: Record<string, string>
  /** 作者共享样式（`core.styles`）。 */
  styles?: string
  /** 作者 Page 样式。 */
  pageStyles?: string
  /** 作者 Widget 样式：widget id → 内容。 */
  widgetStyles?: Record<string, string>
  widgetHtml?: string
  pageHtml?: string
  /** 宿主预编译 Tailwind，与作者样式是两条通道。 */
  widgetCSS?: string
  pageCSS?: string
  i18n?: Record<string, unknown>
  /**
   * 直接安装用包内资源：相对路径 → base64（可带 data-URL 前缀）。
   * 路径必须出现在 `manifest.assets` 中。
   */
  assets?: Record<string, string>
}

/** Manifest 中的 Widget 声明 */
export interface ManifestWidget {
  /** 组件 ID（Tapp 内唯一） */
  id: string
  /** 显示名称 */
  name: string
  description?: string
  /** 图标（emoji 或 icon 名称） */
  icon?: string
  /** 默认尺寸 */
  defaultSize: WidgetSize
  /** 支持的尺寸 */
  sizes: WidgetSize[]
  /** 组件分类 */
  category?: WidgetCategory
  /** 该 widget 的 `.js` 入口；多个 widget 想共用代码就各自 require 同一个文件 */
  entry?: string
  /** 该 widget 的作者样式 */
  styles?: string
  /** HTML 模板文件路径（按尺寸） */
  templates?: Record<string, string>

  /** 每个 Dashboard Widget 实例独立保存的设置 */
  settings?: TappSettingItem[]

  /** 宿主管理的可见性刷新策略 */
  refreshPolicy?: WidgetRefreshPolicy
}

export interface WidgetRefreshPolicy {
  mode: 'event' | 'interval'
  intervalSeconds?: number
  refreshOnVisible?: boolean
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
  description?: string
  /** 默认值 */
  defaultValue?: unknown
  /** select 类型的选项 */
  options?: { value: string; label: string }[]
  /** number 类型的范围 */
  min?: number
  max?: number
  step?: number
  /** input 类型的 placeholder */
  placeholder?: string
}

/** 权限类型 */
export type TappPermission =
  // 小组件权限
  | 'widget:register'
  // 平台数据权限
  | 'platform:read'
  | 'platform:write'
  | 'platform:register'
  // 访问统计（聚合，无访客哈希）
  | 'analytics:read'
  // AI 权限
  | 'ai:generate'
  | 'ai:analyze'
  | 'ai:chat'
  | 'ai:image'
  | '3d:generate'
  // 报告权限
  | 'report:read'
  | 'report:write'
  // 存储权限
  | 'storage:read'
  | 'storage:write'
  // UI 权限
  | 'ui:notification'
  | 'ui:fullscreen'
  | 'ui:theme'
  | 'ui:confirm'
  /** Open host browser tab for a manifest `openUrls` allowlisted link only. */
  | 'ui:openUrl'
  // 网络权限
  | 'network:fetch'
  // 媒体权限
  | 'media:control'
  | 'media:read'
  /** 在沙箱内播放包内/blob/data 音频 */
  | 'media:audio'
  // 组件注册权限
  | 'component:theme'
  | 'component:agent'
  // 快捷键权限
  | 'shortcut:register'
  // 事件权限
  | 'event:publish'
  | 'event:subscribe'
  // P3: 定时任务权限
  | 'scheduler:register'
  // P4: 语音服务权限
  | 'speech:tts'
  | 'speech:asr'
  // Tapp 列表权限
  | 'tappList:read'
  | 'tappList:manage'
  // Brew 权限
  | 'brew:read'
  | 'brew:write'
  | 'brew:commentWrite'
  | 'brew:manage'
  // 联邦权限
  | 'federation:read'
  | 'federation:post'
  | 'federation:interact'
  | 'federation:channel'
  | 'federation:room'
  | 'federation:ring'
  | 'federation:message'
  | 'federation:trust'
  | 'federation:files'
  | 'game:session'

// 用户角色

/** 用户角色类型 */
export type UserRole = 'guest' | 'user' | 'admin'

// 后台运行需求

/** 后台运行需求类型 */
export type BackgroundRequirement =
  | 'media' // 媒体控制（如音乐播放器扩展）
  | 'sync' // 后台数据同步
  | 'notification' // 定时通知
  | 'scheduler' // 定时任务
  | 'event-listener' // 事件监听（跨 Tapp 通信）
  | 'realtime' // 实时数据更新

// Tapp 实例

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

  /** 安装被标记为需重新授权（升级清掉退役权限，或批准列仍有未知名），完成重新授权前不能运行 */
  needsReauthorization?: boolean

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

  /**
   * 公开安装可见性（仅 isAdminTapp 有意义）：
   * - `all`：全体可见
   * - `admin`：仅管理员可见
   */
  visibility?: 'all' | 'admin'

  /**
   * 安装记录在服务端的生命周期状态（不含本页会话假启动）。
   * 公开站主 Tapp 以它为准：`running` 全站可显示；`installed` 表示站长已停，访客不得启动。
   */
  installationStatus?: TappStatus

  /** 错误信息（如果状态为 error） */
  error?: string
}

// 小组件注册

/** 小组件注册配置 */
export interface WidgetRegistration {
  /** 组件 ID（Tapp 内唯一） */
  id: string

  /** 显示名称 */
  name: string

  description?: string

  icon?: string

  /** 支持的尺寸 */
  sizes: WidgetSize[]

  /** 默认尺寸 */
  defaultSize: WidgetSize

  /** 组件分类 */
  category?: WidgetCategory

  /** 每实例设置声明 */
  settings?: TappSettingItem[]

  /** 宿主管理的可见性刷新策略 */
  refreshPolicy?: WidgetRefreshPolicy
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

// 平台数据

/** 平台信息 */
export interface PlatformInfo {
  /** Stable platform slug for cache paths / getData (e.g. "steam") */
  id: string
  /** Same as id — stable key for Tapp SDK consumers */
  key?: string
  /** Display name (e.g. "Steam") */
  name: string
  icon: string
  color: string
  enabled: boolean
  isTappPlatform: boolean
  tappId?: string
  description?: string
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

  icon: string

  /** 主题色 */
  color: string

  description: string

  /** 支持的数据类型 */
  supportedTypes: PlatformDataType[]

  /** URL 模式 */
  urlPattern?: string
}

// AI 相关

/** 服务端权威 AI 用量；null limit/remaining 表示管理员无限制。 */
export interface AIUsageSnapshot {
  calls: {
    limit: number | null
    used: number
    remaining: number | null
    resetsAt: string
  }
  tokens: {
    limit: number | null
    used: number
    remaining: number | null
    resetsAt: string
  }
  cooldown: {
    requiredSeconds: number
    remainingSeconds: number
  }
  restricted: boolean
  restrictionReason?: 'daily_calls' | 'daily_tokens' | 'cooldown'
  unlimited: boolean
  role: UserRole
}

export type AIContextRef =
  | { type: 'platform'; platform: string; selector: string }
  | { type: 'report'; reportId: number }
  | { type: 'profile'; fields: Array<'id' | 'username' | 'role'> }
  | { type: 'custom'; value: unknown }

export interface AIImageInput {
  prompt: string
  width?: number | string
  height?: number | string
  /**
   * Ordered PNG/JPEG/WebP base64 data URLs or /api/brew/image-cache/ paths.
   * At most 4 images, at most 10 MiB of decoded image data in total.
   */
  referenceImages?: string[]
}

interface AITaskRequestOptions {
  version: 2
  context?: AIContextRef[]
  output?: {
    format: TappAIOutputFormat
    schema?: Record<string, unknown>
  }
  delivery?: 'result' | 'stream'
  idempotencyKey?: string
}

export type AITaskRequest = AITaskRequestOptions & (
  | { operation: 'image'; input: string | AIImageInput }
  | { operation: Exclude<TappAIOperation, 'image'>; input: unknown }
)

export type AITaskStatus =
  'queued' | 'running' | 'completed' | 'failed' | 'cancelled'

export interface AITaskSnapshot {
  taskId: string
  status: AITaskStatus
  operation: TappAIOperation
  delivery: 'result' | 'stream'
  createdAt: string
  updatedAt: string
  result?: {
    format: TappAIOutputFormat
    value: unknown
    contextProvenance: unknown[]
  }
  error?: { code: string; message: string }
  usage: AIUsageSnapshot
}

export interface AITaskEvent {
  event:
    | 'snapshot'
    | 'state'
    | 'delta'
    | 'progress'
    | 'result'
    | 'error'
    | 'cancelled'
    | 'resync'
  data: unknown
}

// 消息通信

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

  timestamp: number

  /**
   * iframe → host 消息的会话令牌（request / event 必填）。
   * 仅存在于沙箱发出的消息；host → iframe 的 emit 不携带。
   * 永远不是 Runtime Grant。
   */
  _sessionToken?: string
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
  /** Suggested wait before retry (ms), e.g. quota / mute / rate limit. */
  retryAfter?: number
}
