export const BLOG_BUILD_STATUSES = [
  'idle',
  'queued',
  'running',
  'ok',
  'failed',
  'unavailable',
] as const

export type BlogBuildStatus = (typeof BLOG_BUILD_STATUSES)[number]

export interface BlogSettings {
  theme_id: string
  site_title: string
  description: string
  params: Record<string, unknown>
}

export interface BlogTheme {
  id: string
  name: string
  version?: string
  min_version?: string
  builtin: boolean
  installed_at?: string
  scheme?: BlogThemeScheme
}

export type BlogSchemeTarget = 'html' | 'body'

/** 主题怎么切深浅色，和 crates/myriad-blog/src/scheme.rs 的 `SchemeSwitch` 对应。 */
export type BlogSchemeSwitch =
  | { kind: 'class'; target: BlogSchemeTarget; dark: string; light: string | null }
  | { kind: 'attribute'; target: BlogSchemeTarget; name: string; dark: string; light: string | null }
  | { kind: 'system' }
  | { kind: 'none' }

export interface BlogThemeScheme {
  switch: BlogSchemeSwitch
  /** 让人这么认为的文件，相对主题根目录。 */
  evidence: string[]
  /** 站长确认或改过。 */
  confirmed: boolean
}

export interface BlogHugo {
  available: boolean
  version?: string
}

export interface BlogBuild {
  status: BlogBuildStatus
  last_built_at?: string
  last_error?: string
  log_tail?: string
}

export interface BlogSnapshot {
  settings: BlogSettings
  themes: BlogTheme[]
  hugo: BlogHugo
  build: BlogBuild
}

export interface BlogThemeInstall {
  id: string
  name: string
  version?: string
  min_version?: string
  builtin: boolean
  installed_at?: string
}
