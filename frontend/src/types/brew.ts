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
// - note: 手记源。站长写第一篇时由后端建出来，添加界面里没有这个选项
export type SourceType = 'link' | 'rss' | 'brewlia' | 'rsshub' | 'note'

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
  /** 预定义主题 key（如 "engineering"）；不是展示文案，展示走 i18n。 */
  topic?: string | null
}

/**
 * 用户锁定的磁贴档位（库里是自由 varchar，加值不需要迁移）。
 *
 * - `chip` 竖条 1x2、`bar` 横条 2x1：入口型来源专属
 * - `tiny` 2x2、`mini` 4x2、`full` 4x4：老网格留下的三档，内容磁贴用
 *
 * 档位到尺寸的映射只有一份，在 `components/brew/logic/layout.ts`。
 */
export type CardSize = 'chip' | 'bar' | 'tiny' | 'mini' | 'full'

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
  /**
   * 近两年每篇文章距今天数，最多 60 个，已按新→旧排序。派生字段，不落库。
   * 缺失时节律型磁贴降级为 feature（见 components/brew/logic/layout.ts）。
   */
  pulses?: number[]
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
  /**
   * 预定义主题 key（如 "engineering"），不是展示文案。
   * null / 缺失的文章不参与聚类。关键词或 AI 离线写入，读接口只读已有列。
   */
  topic?: string | null
}

/**
 * 手记的写入载荷。字段名与后端 `NoteWriteRequest` 一一对应。
 *
 * 手记就是 `brew_items` 里的一条，所以写完之后它在阅读器、收藏、评论、
 * 订阅列表里的表现与抓来的文章完全一致。
 */
export interface BrewNoteInput {
  title: string
  /** Markdown 原文。渲染成 HTML 是后端的事，前端不自己解析。 */
  content_md: string
  /** 预定义主题 key；留空表示不参与主题聚类。 */
  topic?: string | null
  /** 封面。不给就取正文里第一张图。 */
  image?: string | null
  /** 发布时间（毫秒）。改稿时不给则保持原值。 */
  published_at?: number | null
}

/** 编辑器读回的那份原文。 */
export interface BrewNoteDraft {
  id: number
  title: string
  content_md: string
  topic: string | null
  image: string | null
  published_at: number
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
  /** 预定义主题 key；与 category 同级过滤，`topic IS NULL` 的文章不入结果 */
  topic?: string
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
}

// 更新订阅源请求
export interface UpdateSourceRequest {
  name?: string
  category?: string
  update_interval?: number
  enabled?: boolean
  /**
   * 磁贴尺寸锁定。空字符串 = 解锁（回到按分数派生），与 theme_color / icon
   * 的清除约定一致。
   */
  card_size?: CardSize | ''
  theme_color?: string
  /** 自定义图标 URL 或 Base64 数据 */
  icon?: string
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
