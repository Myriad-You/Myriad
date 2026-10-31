export type FeedType = 'rss' | 'atom' | 'json_feed' | 'notion' | 'rsshub'

// link: shortcut only, not a subscription.
// note: backend creates on first write; not in the add UI.
export type SourceType = 'link' | 'rss' | 'phantasiai' | 'rsshub' | 'note'

export interface AddSourceInput {
  url: string
  name?: string
  category?: string
  icon?: string
  sourceType?: SourceType
  feedType?: FeedType
  notionToken?: string
}

export interface RSSHubQueryParams {
  limit?: number
  mode?: 'fulltext'
  filter?: string
  filter_title?: string
  filter_description?: string
  filter_author?: string
  /** Seconds. */
  filter_time?: number
  filterout?: string
  filterout_title?: string
  filterout_description?: string
  filterout_author?: string
  filter_case_sensitive?: 0 | 1
  chatgpt?: boolean
  format?: 'rss' | 'atom' | 'json'
  [key: string]: string | number | boolean | undefined
}

export interface RSSHubConfig {
  instanceUrl: string
  routePath: string
  routeParams?: Record<string, string>
  queryParams?: RSSHubQueryParams
  accessKey?: string
}

export interface PhantasiItemPreview {
  id: number
  title: string
  summary: string | null
  image: string | null
  published_at: number | null
  is_read: boolean
  /** Login-only; missing means unstarred. */
  is_starred?: boolean
  /** Topic key, not display copy; display via i18n. */
  topic?: string | null
}

export interface CommentItem {
  id: number
  item_id: number
  user_id: number
  user_name?: string
  user_display_name?: string
  user_avatar?: string
  selected_text: string
  comment: string
  start_offset?: number
  end_offset?: number
  context_before?: string
  context_after?: string
  color?: string
  is_public: boolean
  parent_id?: number
  content_revision?: number
  created_at: number
  updated_at: number
  replies?: CommentItem[]
  reply_count?: number
  item_title?: string
  source_id?: number
  source_name?: string
}

/** bar is entry-source only; tiny/mini/full for content. Mapping in layout.ts. DB is free varchar. */
export type CardSize = 'bar' | 'tiny' | 'mini' | 'full'

export interface PhantasiSource {
  id: number
  /** 共享目录不再回创建者；旧包/夹具可能仍带。 */
  user_id?: number
  name: string
  url: string
  feed_type: FeedType
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
  ai_style_tags: string[] | null
  /** Set only when feed_type is rsshub. */
  rsshub_route: string | null
  admin_only: boolean
  created_at: number
  recent_items?: PhantasiItemPreview[]
  /** Derived, not stored. Missing → feature fallback (layout.ts). */
  pulses?: number[]
}

export interface PhantasiItem {
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
  /** 0 means no user state exists yet. */
  state_revision?: number
  content_revision?: number
  created_at: number
  has_ai_annotations?: boolean
  has_ai_podcast?: boolean
  /** AI web search, not a DB article. */
  fromWebSearch?: boolean
  /** Topic key, not display copy. Null/missing items are not clustered. Read APIs are read-only. */
  topic?: string | null
}

/** Fields match backend NoteWriteRequest. */
export interface PhantasiNoteInput {
  title: string
  /** Markdown; backend renders HTML. */
  content_md: string
  /** Empty = not clustered. */
  topic?: string | null
  /** First body image if omitted. */
  image?: string | null
  /** Omit on edit to keep the previous value. */
  published_at?: number | null
}

type PhantasiNoteDocStatus = 'draft' | 'scheduled' | 'published'

export interface PhantasiNoteAuthor {
  user_id: number
  user_name?: string
  user_display_name?: string
  role: string
}

export interface PhantasiNoteDoc {
  id: number
  item_id: number | null
  user_id?: number
  user_name?: string
  user_display_name?: string
  authors?: PhantasiNoteAuthor[]
  title: string
  content_md: string
  has_body?: boolean
  excerpt?: string | null
  topic: string | null
  image: string | null
  /** 公开地址 `/blog/posts/{slug}/` 的那一段。空则发布时由后端生成。 */
  slug?: string | null
  /** 搬家来的文章原来的地址（规范化后的路径），访问会 301 到这篇。只有单篇读取的已发布笔记带。 */
  old_paths?: string[]
  status: PhantasiNoteDocStatus
  scheduled_at: number | null
  published_at: number | null
  revision: number
  last_error?: string | null
  updated_at: number
}

export interface PhantasiNoteDocInput {
  client_request_id?: string
  title?: string
  content_md?: string
  topic?: string | null
  image?: string | null
  slug?: string | null
  /** 整组替换；省略表示不改。只有已发布的笔记（或发布这一下）能带。 */
  old_paths?: string[]
  published_at?: number | null
  scheduled_at?: number | null
  revision?: number
}

export interface PhantasiCategory {
  id: number
  user_id?: number
  name: string
  icon: string | null
  color: string | null
  sort_order: number
  created_at: number
}

export interface PhantasiStats {
  total_sources: number
  total_items: number
  total_unread: number
  total_starred: number
}

export interface PhantasiSourcesResponse {
  success: boolean
  sources: PhantasiSource[]
  error?: string
}

export interface PhantasiItemsResponse {
  success: boolean
  items: Omit<PhantasiItem, 'content' | 'has_ai_annotations' | 'has_ai_podcast'>[]
  total: number
  page: number
  per_page: number
  next_cursor?: string | null
  error?: string
}

export interface PhantasiCategoriesResponse {
  success: boolean
  categories: PhantasiCategory[]
  error?: string
}

export interface PhantasiStatsResponse {
  success: boolean
  stats: PhantasiStats
  error?: string
}

export interface PhantasiItemsQuery {
  source_id?: number
  category?: string
  /** Topic name; `topic IS NULL` rows are excluded. */
  topic?: string
  filter?: 'all' | 'unread' | 'starred'
  sort_order?: 'asc' | 'desc'
  page?: number
  per_page?: number
  cursor?: string
}

export interface AddSourceRequest {
  url: string
  name?: string
  category?: string
  update_interval?: number
  icon?: string
  source_type?: SourceType
  feed_type?: FeedType
  /** Set only when feed_type is rsshub. */
  rsshub_route?: string
  extra_config?: {
    token?: string
    filter?: unknown
    sort?: unknown
  }
  admin_only?: boolean
  description?: string
  site_url?: string
  enabled?: boolean
  sort_order?: number
}

export interface UpdateSourceRequest {
  name?: string
  category?: string
  update_interval?: number
  enabled?: boolean
  /** Empty string unlocks (same clear convention as theme_color / icon). */
  card_size?: CardSize | ''
  theme_color?: string
  icon?: string
  description?: string
  site_url?: string
  sort_order?: number
  /** Subscription URL; often changes with source/feed type. */
  url?: string
  source_type?: SourceType
  feed_type?: FeedType
  /** Only when feed_type is rsshub. Empty string clears. */
  rsshub_route?: string
  extra_config?: {
    token?: string
    filter?: unknown
    sort?: unknown
    rsshub?: RSSHubConfig
  }
  ai_style_tags?: string[]
  admin_only?: boolean
}

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

export interface PipackCategory {
  name: string
  icon: string | null
  color: string | null
  sort_order: number
}

export interface PipackRsshubInstance {
  name: string
  url: string
  priority: number
  enabled: boolean
}

export interface PhantasiExportManifest {
  version: string
  exported_at: string
  sources: PipackSource[]
  categories?: PipackCategory[]
  rsshub_instances?: PipackRsshubInstance[]
}

export interface PipackSource {
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

export type PhantasiApplicationStatus = 'pending' | 'approved' | 'rejected'

export interface PhantasiSourceApplication {
  id: number
  kind: string
  status: PhantasiApplicationStatus | string
  site_name: string
  site_url: string
  feed_url?: string | null
  description?: string | null
  message?: string | null
  applicant_name?: string | null
  applicant_email?: string | null
  applicant_user_id?: number | null
  applicant_ip?: string | null
  result_source_id?: number | null
  review_note?: string | null
  reviewed_by?: number | null
  reviewed_at?: number | null
  created_at: number
  updated_at: number
  has_feed: boolean
}

export interface ApplySourceApplicationInput {
  site_name: string
  site_url: string
  feed_url?: string
  description?: string
  message?: string
  applicant_name?: string
  applicant_email?: string
}
