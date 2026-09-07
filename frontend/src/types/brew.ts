/**
 * Brew 阅读系统类型定义
 */

// 订阅源类型（内容来源格式）
// - rss: RSS 格式订阅
// - atom: Atom 格式订阅
// - json_feed: JSON Feed 格式订阅
// - notion: Notion 数据库/页面订阅
// - rsshub: RSSHub 动态路由订阅
export type FeedType = 'rss' | 'atom' | 'json_feed' | 'notion' | 'rsshub'

// 来源类型（订阅模式）
// - link: 纯链接，不订阅，仅作为快捷入口
// - rss: 标准订阅（适用于 RSS/Atom/JSON Feed/Notion）
// - brewlia: AI 增强订阅（适用于 RSS 和 Notion），提供词汇注释等增强功能
// - rsshub: RSSHub 订阅（支持多实例切换，独立于传统订阅）
export type SourceType = 'link' | 'rss' | 'brewlia' | 'rsshub'

/** 从 Brew 添加界面提交的订阅源参数。 */
export interface AddSourceInput {
  url: string
  name?: string
  category?: string
  icon?: string
  sourceType?: SourceType
  feedType?: FeedType
  notionToken?: string
}

// RSSHub 实例配置

// RSSHub 通用查询参数（适用于大多数路由）
export interface RSSHubQueryParams {
  /** 条目数量限制 */
  limit?: number
  /** 全文输出模式（部分路由支持） */
  mode?: 'fulltext'
  /** 内容过滤 - 仅保留匹配的条目（正则或关键词） */
  filter?: string
  /** 标题过滤 - 仅保留匹配的条目 */
  filter_title?: string
  /** 描述过滤 - 仅保留匹配的条目 */
  filter_description?: string
  /** 作者过滤 */
  filter_author?: string
  /** 时间过滤 - 仅保留指定秒数内的条目 */
  filter_time?: number
  /** 内容排除 - 排除匹配的条目 */
  filterout?: string
  /** 标题排除 */
  filterout_title?: string
  /** 描述排除 */
  filterout_description?: string
  /** 作者排除 */
  filterout_author?: string
  /** 是否使用正则匹配（0 或 1） */
  filter_case_sensitive?: 0 | 1
  /** OpenAI 总结（需实例支持） */
  chatgpt?: boolean
  /** 输出格式 */
  format?: 'rss' | 'atom' | 'json'
  /** 自定义查询参数（键值对） */
  [key: string]: string | number | boolean | undefined
}

// RSSHub 订阅额外配置
export interface RSSHubConfig {
  /** 当前使用的实例 URL */
  instanceUrl: string
  /** 路由路径（不含实例 URL） */
  routePath: string
  /** 路由参数（路径中的 :param） */
  routeParams?: Record<string, string>
  /** 查询参数（?key=value） */
  queryParams?: RSSHubQueryParams
  /** 访问密钥（用于私有实例鉴权） */
  accessKey?: string
}

// 最新文章预览（用于卡片显示）
export interface BrewItemPreview {
  id: number
  title: string
  summary: string | null
  image: string | null
  published_at: number | null
  is_read: boolean
}

// 卡片尺寸类型：full(4行) | mini(2行) | tiny(1行)
export type CardSize = 'full' | 'mini' | 'tiny'

// 订阅源
export interface BrewSource {
  id: number
  user_id: number
  name: string
  url: string
  feed_type: FeedType
  /** 来源类型: link(纯链接), rss(RSS订阅), brewlia(AI增强订阅) */
  source_type: SourceType
  category: string | null
  icon: string | null
  description: string | null
  site_url: string | null
  update_interval: number
  last_fetched_at: number | null
  last_success_at: number | null
  last_error: string | null
  error_count: number
  enabled: boolean
  item_count: number
  unread_count: number
  card_size: CardSize | null
  theme_color: string | null
  sort_order: number | null
  /** AI 风格标签 */
  ai_style_tags: string[] | null
  /** RSSHub 路由路径（仅当 feed_type = rsshub 时有值） */
  rsshub_route: string | null
  /** 仅管理员可见 */
  admin_only: boolean
  created_at: number
  // 最新文章预览（最多3篇）
  recent_items?: BrewItemPreview[]
}

// 文章项
export interface BrewItem {
  id: number
  source_id: number
  source_name: string | null
  source_icon: string | null
  guid: string
  title: string
  link: string
  summary: string | null
  content: string | null
  image: string | null
  audio_url: string | null
  author: string | null
  published_at: number | null
  word_count: number | null
  reading_time: number | null
  is_read: boolean
  is_starred: boolean
  read_progress: number | null
  created_at: number
  // AI 功能状态
  /** 是否已生成 AI 注释 */
  has_ai_annotations?: boolean
  /** 是否已生成 AI 播客 */
  has_ai_podcast?: boolean
  /** 是否来自 AI 网络搜索（非数据库文章） */
  fromWebSearch?: boolean
}

// 分类
export interface BrewCategory {
  id: number
  user_id: number
  name: string
  icon: string | null
  color: string | null
  sort_order: number
  created_at: number
}

// 统计信息
export interface BrewStats {
  total_sources: number
  total_items: number
  total_unread: number
  total_starred: number
}

// API 响应类型
export interface BrewSourcesResponse {
  success: boolean
  sources: BrewSource[]
  error?: string
}

export interface BrewItemsResponse {
  success: boolean
  items: BrewItem[]
  total: number
  page: number
  per_page: number
  error?: string
}

export interface BrewCategoriesResponse {
  success: boolean
  categories: BrewCategory[]
  error?: string
}

export interface BrewStatsResponse {
  success: boolean
  stats: BrewStats
  error?: string
}

// 筛选参数
export interface BrewItemsQuery {
  source_id?: number
  category?: string
  filter?: 'all' | 'unread' | 'starred'
  sort_order?: 'asc' | 'desc'
  page?: number
  per_page?: number
}

// 添加订阅源请求
export interface AddSourceRequest {
  url: string
  name?: string
  category?: string
  update_interval?: number
  /** 自定义图标 URL 或 Base64 数据 */
  icon?: string
  /** 来源类型: link, rss, brewlia, rsshub */
  source_type?: SourceType
  /** 订阅源类型：rss, atom, json_feed, notion, rsshub */
  feed_type?: FeedType
  /** RSSHub 路由路径（仅当 feed_type = rsshub 时使用） */
  rsshub_route?: string
  /** 额外配置（如 Notion token） */
  extra_config?: {
    token?: string
    filter?: unknown
    sort?: unknown
  }
  /** 仅管理员可见 */
  admin_only?: boolean
  description?: string
  site_url?: string
  enabled?: boolean
  sort_order?: number
}

// 更新订阅源请求
export interface UpdateSourceRequest {
  name?: string
  category?: string
  update_interval?: number
  enabled?: boolean
  card_size?: CardSize
  theme_color?: string
  /** 自定义图标 URL 或 Base64 数据 */
  icon?: string
  description?: string
  site_url?: string
  /** 自定义排序顺序 */
  sort_order?: number
  /** 来源类型: link, rss, brewlia, rsshub */
  source_type?: SourceType
  /** 订阅源类型：rss, atom, json_feed, notion, rsshub */
  feed_type?: FeedType
  /** 额外配置（如 Notion token, RSSHub config） */
  extra_config?: {
    token?: string
    filter?: unknown
    sort?: unknown
    /** RSSHub 配置 */
    rsshub?: RSSHubConfig
  }
  /** AI 风格标签（用户自定义或 AI 生成） */
  ai_style_tags?: string[]
  /** 仅管理员可见 */
  admin_only?: boolean
}

// 创建分类请求
export interface CreateCategoryRequest {
  name: string
  icon?: string
  color?: string
}

export interface UpdateCategoryRequest {
  name?: string
  icon?: string
  color?: string
  sort_order?: number
}

export interface RsshubInstance {
  id: number
  user_id: number | null
  name: string
  url: string
  has_access_key: boolean
  priority: number
  enabled: boolean
  health_status: 'healthy' | 'degraded' | 'unhealthy' | 'unknown'
  last_health_check: number | null
  last_response_time_ms: number | null
  consecutive_failures: number
  success_rate: number
  created_at: number
}

export interface AddRsshubInstanceRequest {
  name: string
  url: string
  access_key?: string | null
  priority?: number
}

export interface UpdateRsshubInstanceRequest {
  name?: string
  url?: string
  access_key?: string | null
  priority?: number
  enabled?: boolean
}

export interface BrewpackCategory {
  name: string
  icon: string | null
  color: string | null
  sort_order: number
}

export interface BrewpackRsshubInstance {
  name: string
  url: string
  priority: number
  enabled: boolean
}

/** Brew 订阅包清单（v2 含分类实体与 RSSHub 实例） */
export interface BrewExportManifest {
  version: string
  exported_at: string
  sources: BrewpackSource[]
  categories?: BrewpackCategory[]
  rsshub_instances?: BrewpackRsshubInstance[]
}

export interface BrewpackSource {
  url: string
  name: string
  category: string | null
  icon_file: string | null
  icon_url: string | null
  source_type: SourceType
  feed_type: FeedType
  theme_color: string | null
  update_interval: number
  card_size: string | null
  rsshub_route: string | null
  ai_style_tags: string[] | null
  admin_only: boolean
  description: string | null
  site_url: string | null
  enabled: boolean
  sort_order: number | null
}
