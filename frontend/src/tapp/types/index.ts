export type PermissionLevel = 'public' | 'basic' | 'elevated' | 'privileged'

export type TappStatus = 'installed' | 'running' | 'suspended' | 'error'

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

export type PlatformDataType =
  'game' | 'video' | 'music' | 'anime' | 'article' | 'custom'

/** 用途分类；Page / Widget 等运行形态不在此字段。 */
export type TappCategory =
  | 'ai'
  | 'data'
  | 'developer'
  | 'game'
  | 'media'
  | 'productivity'
  | 'social'
  | 'utility'

/** 与 TappCategory 同一套稳定 ID。 */
export type WidgetCategory = TappCategory

export interface TappManifestLocaleEntry {
  name?: string
  description?: string
}

/** BCP-47 → 展示文案；未命中回退顶层 name/description。 */
export type TappManifestLocales = Record<string, TappManifestLocaleEntry>

/** 共享层：三种运行形态先执行；headless 后台只执行它。 */
export interface TappCoreLayer {
  /** 层入口；层内其余文件由入口 require。 */
  entry: string
  /** 作者样式；宿主预编译产物不走这里。 */
  styles?: string
}

/** Page 层。entry 与 template 至少有一个。 */
export interface TappPageLayer {
  entry?: string
  template?: string
  styles?: string
}

export interface TappManifest {
  id: string

  name: string

  version: string

  description?: string

  locales?: TappManifestLocales

  author?: {
    name: string
    email?: string
    url?: string
  }

  icon?: string

  iconSvg?: string

  /** 全彩图默认 standalone；iconShell true 则仍套 material 壳。 */
  iconShell?: boolean

  themeColor?: string

  permissions: TappPermission[]

  homepage?: string

  repository?: string

  minSystemVersion?: string

  widgets?: ManifestWidget[]

  /** 共享层。声明 backgroundRequirements 时必须有。 */
  core?: TappCoreLayer

  /** 声明即有可打开页面。 */
  page?: TappPageLayer

  /** 声明后由 TappBackgroundRunner 拉起 headless core。 */
  backgroundRequirements?: BackgroundRequirement[]

  runtimeModules?: Array<'three'>

  /** Needs game:session plus a federation permission. */
  game?: {
    protocol: string
    maxPlayers?: number
    maxMessageBytes?: number
  }

  category: TappCategory

  settings?: TappSettingItem[]

  /** 安装级只写凭据；值只绑定到声明 HTTP API。 */
  credentials?: TappCredentialItem[]

  apis?: Record<string, TappApiDefinition>

  /** 读取每次需宿主授权弹窗。 */
  dataExchange?: TappDataExchangeManifest

  ai?: TappAIManifest

  events?: TappEventsManifest

  agent?: TappAgentManifest

  /** 须在 assets/ 下；走 Tapp.assets，不走 Tapp.storage。 */
  assets?: string[]

  /** 只开声明 id；未声明 URL 拒绝。需要 ui:openUrl。 */
  openUrls?: TappOpenUrlDef[]
}

export interface TappOpenUrlDef {
  id: string
  url: string
  /** exact 仅声明 URL；prefix 同 origin 且路径在前缀下；origin 该源任意路径。 */
  match?: 'exact' | 'prefix' | 'origin'
}

export type TappAIOperation = 'generate' | 'analyze' | 'chat' | 'image' | 'search'
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
  /** 内联 JSON Schema 子集；不支持 $ref。 */
  schema: Record<string, unknown>
  maxBytes: number
  maxRecords?: number
  description?: string
}

export interface TappDataImport {
  tappId: string
  exportId: string
}

export interface TappApiDefinition {
  /** public 游客；protected 登录；manager owner/管理员。与 network:fetch 无关——type:http 仍须 network:fetch。 */
  access?: 'public' | 'protected' | 'manager'
  type?: 'http' | 'builtin'
  endpoint?: string
  method?: string
  headers?: Record<string, string>
  /** 宿主凭据绑定；密钥不进模板上下文。 */
  credential?: {
    key: string
    /** 省略且声明 header 时视为 header。 */
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
  bodyMode?: 'json' | 'raw' | 'form'
  body?: unknown
  builtin?: string
  inject?: Record<string, string>
  cacheTtl?: number
  spoof?: string
  description?: string
}

export interface TappCredentialItem {
  key: string
  label: string
  description?: string
  placeholder?: string
}

export interface TappCodeStructure {
  /** 只含当前模式依赖图。 */
  modules: Record<string, string>
  /** 宿主预解析的 require 图；缺失时由前端扫描。 */
  moduleResolutions?: Record<string, Record<string, string>>
  /** core 层入口；headless 后台只执行它。 */
  coreEntry?: string
  pageEntry?: string
  /** widget id → 层入口。 */
  widgetEntries?: Record<string, string>
  styles?: string
  pageStyles?: string
  widgetStyles?: Record<string, string>
  widgetHtml?: string
  pageHtml?: string
  /** 宿主预编译 Tailwind，与作者样式两条通道。 */
  widgetCSS?: string
  pageCSS?: string
  i18n?: Record<string, unknown>
  /** 路径必须出现在 manifest.assets。 */
  assets?: Record<string, string>
}

export interface ManifestWidget {
  id: string
  name: string
  description?: string
  icon?: string
  defaultSize: WidgetSize
  sizes: WidgetSize[]
  category?: WidgetCategory
  /** 该 widget 的层入口；共用代码则各自 require 同一文件。 */
  entry?: string
  styles?: string
  templates?: Record<string, string>

  settings?: TappSettingItem[]

  refreshPolicy?: WidgetRefreshPolicy
}

export interface WidgetRefreshPolicy {
  mode: 'event' | 'interval'
  intervalSeconds?: number
  refreshOnVisible?: boolean
}

export type TappSettingType = 'toggle' | 'select' | 'input' | 'number' | 'color'

export interface TappSettingItem {
  key: string
  label: string
  type: TappSettingType
  description?: string
  defaultValue?: unknown
  options?: { value: string; label: string }[]
  min?: number
  max?: number
  step?: number
  placeholder?: string
  multiline?: boolean
  rows?: number
}

export type TappPermission =
  | 'widget:register'
  | 'platform:read'
  | 'platform:write'
  | 'platform:register'
  | 'analytics:read'
  | 'ai:generate'
  | 'ai:analyze'
  | 'ai:chat'
  | 'ai:image'
  | 'ai:search'
  | '3d:generate'
  | 'report:read'
  | 'report:write'
  | 'storage:read'
  | 'storage:write'
  | 'ui:notification'
  | 'ui:fullscreen'
  | 'ui:theme'
  | 'ui:confirm'
  /** 仅打开 manifest openUrls 声明的链接。 */
  | 'ui:openUrl'
  | 'network:fetch'
  | 'media:control'
  | 'media:read'
  /** 沙箱内播放包内/blob/data 音频。 */
  | 'media:audio'
  | 'component:theme'
  | 'component:agent'
  | 'shortcut:register'
  | 'event:publish'
  | 'event:subscribe'
  | 'scheduler:register'
  | 'speech:tts'
  | 'speech:asr'
  | 'tappList:read'
  | 'tappList:manage'
  | 'brew:read'
  | 'brew:write'
  | 'brew:commentWrite'
  | 'brew:manage'
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

export type UserRole = 'guest' | 'user' | 'admin'

export type BackgroundRequirement =
  | 'media'
  | 'sync'
  | 'notification'
  | 'scheduler'
  | 'event-listener'
  | 'realtime'

export interface TappInstance {
  id: string

  manifest: TappManifest

  status: TappStatus

  installedAt: string

  lastRunAt?: string

  /** 授予权限。 */
  grantedPermissions: TappPermission[]

  /** 升级清掉退役权限或批准列仍有未知名时置位；完成重新授权前不能运行。 */
  needsReauthorization?: boolean

  userRole: UserRole

  isTemporary?: boolean

  /** Playground 预览。无 Runtime Grant；不等于安装后的授予权限。与 isTemporary 不是同一层。 */
  previewMode?: boolean

  isAdminTapp?: boolean

  /** 公开安装：all 全体，admin 仅管理员。仅 isAdminTapp。 */
  visibility?: 'all' | 'admin'

  /** 服务端安装生命周期，不含本页会话假启动。公开站主 Tapp 以它为准。 */
  installationStatus?: TappStatus

  error?: string
}

export interface WidgetRegistration {
  id: string

  name: string

  description?: string

  icon?: string

  sizes: WidgetSize[]

  defaultSize: WidgetSize

  category?: WidgetCategory

  settings?: TappSettingItem[]

  refreshPolicy?: WidgetRefreshPolicy
}

export interface RegisteredWidget {
  id: string

  tappId: string

  config: WidgetRegistration

  instanceCount: number

  registeredAt: string
}

export interface WidgetRenderProps {
  size: WidgetSize

  config: Record<string, unknown>

  isEditMode: boolean

  isPreview: boolean

  scale: number

  fontScale: number

  theme: 'light' | 'dark'

  primaryColor?: string

  locale?: string
}

export interface PlatformInfo {
  /** 稳定 slug，给 cache / getData。 */
  id: string
  /** 同 id；给 SDK。 */
  key?: string
  name: string
  icon: string
  color: string
  enabled: boolean
  isTappPlatform: boolean
  tappId?: string
  description?: string
}

export interface NewPlatformItem {
  platform: string

  type: PlatformDataType

  title: string

  cover?: string

  description?: string

  url?: string

  metadata?: Record<string, unknown>

  createdAt?: string
}

export interface PlatformItemResult {
  success: boolean
  itemId: string
  source: string
}

export interface CustomPlatformConfig {
  id: string

  name: string

  icon: string

  color: string

  description: string

  supportedTypes: PlatformDataType[]

  urlPattern?: string
}

/** null limit/remaining = 管理员无限制。 */
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
  /** 最多 4 张；解码后合计最多 10 MiB。 */
  referenceImages?: string[]
}

export interface AISearchInput {
  query: string
  searchType?: 'rss_source' | 'api_docs' | 'general'
  maxResults?: number
  searchPrompt?: string
}

export interface AIGenerateInput {
  prompt: string
}

export interface AIAnalyzeInput {
  data: unknown
  instruction?: string
}

export interface AIChatMessage {
  role: 'system' | 'user' | 'assistant'
  content: string
}

export interface AIChatInput {
  messages: AIChatMessage[]
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
  | { operation: 'search'; input: string | AISearchInput }
  | { operation: 'generate'; input: string | AIGenerateInput }
  | { operation: 'analyze'; input: AIAnalyzeInput }
  | { operation: 'chat'; input: AIChatInput }
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

export type TappMessageType = 'request' | 'response' | 'event'

export interface TappMessage<T = unknown> {
  type: TappMessageType

  id: string

  action: string

  payload: T

  source?: string

  timestamp: number

  /** iframe→host 会话令牌；永远不是 Runtime Grant。host→iframe emit 不携带。 */
  _sessionToken?: string
}

export interface TappAPIRequest {
  api: string

  method: string

  args: unknown[]
}

export interface TappAPIResponse<T = unknown> {
  success: boolean
  data?: T
  error?: string
  code?: string
  retryAfter?: number
}
