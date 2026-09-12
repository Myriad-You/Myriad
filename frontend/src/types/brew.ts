export type FeedType = 'rss' | 'atom' | 'json_feed' | 'notion' | 'rsshub'

// link: shortcut only, not a subscription.
// note: backend creates on first write; not in the add UI.
export type SourceType = 'link' | 'rss' | 'brewlia' | 'rsshub' | 'note'

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

export interface BrewItemPreview {
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

/** bar is entry-source only; tiny/mini/full for content. Mapping in layout.ts. DB is free varchar. */
export type CardSize = 'bar' | 'tiny' | 'mini' | 'full'

export interface BrewSource {
  id: number
  user_id: number
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
  recent_items?: BrewItemPreview[]
  /** Derived, not stored. Missing → feature fallback (layout.ts). */
  pulses?: number[]
}

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
export interface BrewNoteInput {
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

export interface BrewNoteDraft {
  id: number
  title: string
  content_md: string
  topic: string | null
  image: string | null
  published_at: number
}

export interface BrewCategory {
  id: number
  user_id: number
  name: string
  icon: string | null
  color: string | null
  sort_order: number
  created_at: number
}

export interface BrewStats {
  total_sources: number
  total_items: number
  total_unread: number
  total_starred: number
}

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
  next_cursor?: string | null
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

export interface BrewItemsQuery {
  source_id?: number
  category?: string
  /** Topic key; `topic IS NULL` rows are excluded. */
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
  source_type?: SourceType
  feed_type?: FeedType
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
