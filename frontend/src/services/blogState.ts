import type {
  BlogBuild,
  BlogBuildStatus,
  BlogHugo,
  BlogSchemeSwitch,
  BlogSettings,
  BlogSnapshot,
  BlogTheme,
  BlogThemeScheme,
} from '../types/blog'
import { BLOG_BUILD_STATUSES } from '../types/blog'

/** 与 `crates/myriad-blog` 的 `DEFAULT_THEME_ID` 相同。内置主题不能卸载。 */
export const DEFAULT_BLOG_THEME_ID = 'myriad-default'

const STATUS_SET = new Set<string>(BLOG_BUILD_STATUSES)

export function blogBuildStatus(value: unknown): BlogBuildStatus {
  return typeof value === 'string' && STATUS_SET.has(value)
    ? (value as BlogBuildStatus)
    : 'idle'
}

/** queued / running 才轮询。停在失败或不可用，避免空转。 */
export function blogBuildPolls(status: BlogBuildStatus): boolean {
  return status === 'queued' || status === 'running'
}

export function blogThemeCanUninstall(theme: { id?: string; builtin: boolean }): boolean {
  return !theme.builtin && theme.id !== DEFAULT_BLOG_THEME_ID
}

/** 卸掉正在用的主题时，后端会切回内置主题再重建。 */
export function blogThemeRemovalReturnsToDefault(themeId: string, currentThemeId: string): boolean {
  return themeId === currentThemeId
}

/** 主题参数必须是 JSON 对象，数组和标量都不算。 */
export function parseBlogParams(text: string): Record<string, unknown> | null {
  const trimmed = text.trim()
  if (!trimmed) return {}
  try {
    const parsed: unknown = JSON.parse(trimmed)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    return parsed as Record<string, unknown>
  } catch {
    return null
  }
}

export function blogThemeInstallUrl(raw: string): string | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  try {
    const url = new URL(trimmed)
    if (url.protocol !== 'https:' && url.protocol !== 'http:') return null
    return url.toString()
  } catch {
    return null
  }
}

/** `hugo version` 的整行输出带提交号和构建时间，界面只要版本号和是否 extended。 */
export function blogHugoVersionLabel(raw: string): string {
  const match = /\bv?(\d+\.\d+\.\d+)(\S*)/.exec(raw)
  if (!match) return raw.trim()
  return match[2]?.includes('+extended') ? `v${match[1]} extended` : `v${match[1]}`
}

export function sameBlogSettings(a: BlogSettings, b: BlogSettings): boolean {
  return (
    a.theme_id === b.theme_id &&
    a.site_title === b.site_title &&
    a.description === b.description &&
    JSON.stringify(a.params) === JSON.stringify(b.params)
  )
}

function text(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

const SCHEME_TOKEN = /^[\w-]{1,40}$/
const SCHEME_DATA_ATTR = /^data-[a-z0-9-]{1,40}$/

/** 和后端 `SchemeSwitch::is_valid` 一样：这些字会原样写进博客页。 */
export function blogSchemeSwitchValid(value: BlogSchemeSwitch): boolean {
  if (value.kind === 'system' || value.kind === 'none') return true
  const lightOk = value.light == null || (SCHEME_TOKEN.test(value.light) && value.light !== value.dark)
  if (!SCHEME_TOKEN.test(value.dark) || !lightOk) return false
  return value.kind === 'class' || SCHEME_DATA_ATTR.test(value.name)
}

export function blogSchemeSwitchFrom(value: unknown): BlogSchemeSwitch | null {
  if (!value || typeof value !== 'object') return null
  const row = value as Record<string, unknown>
  const target = row.target === 'body' ? 'body' : row.target === 'html' ? 'html' : null
  const light = typeof row.light === 'string' && row.light ? row.light : null
  let parsed: BlogSchemeSwitch | null = null
  if (row.kind === 'system' || row.kind === 'none') { parsed = { kind: row.kind }
}
  else if (row.kind === 'class' && target) { parsed = { kind: 'class', target, dark: text(row.dark), light }
}
  else if (row.kind === 'attribute' && target) {
    parsed = { kind: 'attribute', target, name: text(row.name), dark: text(row.dark), light }
  }
  return parsed && blogSchemeSwitchValid(parsed) ? parsed : null
}

function schemeFrom(value: unknown): BlogThemeScheme | null {
  if (!value || typeof value !== 'object') return null
  const row = value as Record<string, unknown>
  const switchValue = blogSchemeSwitchFrom(row.switch)
  if (!switchValue) return null
  const evidence = Array.isArray(row.evidence)
    ? row.evidence.filter((item): item is string => typeof item === 'string')
    : []
  return { switch: switchValue, evidence, confirmed: row.confirmed === true }
}

/** 写在 `<html>` / `<body>` 上的样子，给人看的：`<html class="dark">`。 */
export function blogSchemeMarkup(value: BlogSchemeSwitch, dark: boolean): string | null {
  if (value.kind === 'system' || value.kind === 'none') return null
  const token = dark ? value.dark : value.light
  const name = value.kind === 'class' ? 'class' : value.name
  return token ? `<${value.target} ${name}="${token}">` : `<${value.target}>`
}

export function blogThemeFromValue(value: unknown): BlogTheme | null {
  return themeFrom(value)
}

function themeFrom(value: unknown): BlogTheme | null {
  if (!value || typeof value !== 'object') return null
  const row = value as Record<string, unknown>
  const id = text(row.id).trim()
  if (!id) return null
  const theme: BlogTheme = {
    id,
    name: text(row.name).trim() || id,
    builtin: row.builtin === true,
  }
  if (typeof row.version === 'string' && row.version) theme.version = row.version
  if (typeof row.min_version === 'string' && row.min_version) theme.min_version = row.min_version
  if (typeof row.installed_at === 'string' && row.installed_at) theme.installed_at = row.installed_at
  const scheme = schemeFrom(row.scheme)
  if (scheme) theme.scheme = scheme
  return theme
}

function paramsFrom(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
  return value as Record<string, unknown>
}

/** 缺字段走空标题和 idle，避免半截响应把面板打崩。 */
export function blogSnapshotFromValue(value: unknown): BlogSnapshot {
  const root = value && typeof value === 'object' ? (value as Record<string, unknown>) : {}
  const settingsRaw = root.settings && typeof root.settings === 'object'
    ? (root.settings as Record<string, unknown>)
    : {}
  const hugoRaw = root.hugo && typeof root.hugo === 'object'
    ? (root.hugo as Record<string, unknown>)
    : {}
  const buildRaw = root.build && typeof root.build === 'object'
    ? (root.build as Record<string, unknown>)
    : {}
  const settings: BlogSettings = {
    theme_id: text(settingsRaw.theme_id),
    site_title: text(settingsRaw.site_title),
    description: text(settingsRaw.description),
    params: paramsFrom(settingsRaw.params),
  }
  const hugo: BlogHugo = { available: hugoRaw.available === true }
  if (typeof hugoRaw.version === 'string' && hugoRaw.version) hugo.version = hugoRaw.version
  const build: BlogBuild = { status: blogBuildStatus(buildRaw.status) }
  if (typeof buildRaw.last_built_at === 'string' && buildRaw.last_built_at) {
    build.last_built_at = buildRaw.last_built_at
  }
  if (typeof buildRaw.last_error === 'string' && buildRaw.last_error) {
    build.last_error = buildRaw.last_error
  }
  if (typeof buildRaw.log_tail === 'string' && buildRaw.log_tail) {
    build.log_tail = buildRaw.log_tail
  }
  const themes = Array.isArray(root.themes)
    ? root.themes.map(themeFrom).filter((theme): theme is BlogTheme => theme != null)
    : []
  return { settings, themes, hugo, build }
}
