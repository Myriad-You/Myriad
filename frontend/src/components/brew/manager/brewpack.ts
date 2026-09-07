/**
 * Brew 订阅包（.brewpack）清单：导出字段与导入写回同源。
 * Notion token / RSSHub access_key 不进包。
 */
import type {
  AddSourceRequest,
  BrewCategory,
  BrewExportManifest,
  BrewpackCategory,
  BrewpackRsshubInstance,
  BrewpackSource,
  BrewSource,
  CardSize,
  RsshubInstance,
  SourceType,
  UpdateSourceRequest,
} from '../../../types/brew'

export const BREWPACK_VERSION = '2.0'
export const BREWPACK_LEGACY_VERSION = '1.0'

export function isSupportedBrewpackVersion(version: string): boolean {
  return version === BREWPACK_VERSION || version === BREWPACK_LEGACY_VERSION
}

export function normalizeBrewpackUrl(url: string): string {
  return url.trim().replace(/\/+$/, '')
}

const DATA_IMAGE_RE =
  /^data:(image\/[a-z0-9.+-]+)(?:;[\w.=+-]+)*;base64,/i

export function isDataImageUrl(value: string | null | undefined): boolean {
  return typeof value === 'string' && /^data:image\//i.test(value.trim())
}

function mimeToIconExt(mime: string): string {
  const subtype = mime.slice('image/'.length)
  switch (subtype) {
    case 'svg+xml':
    case 'svg':
      return 'svg'
    case 'jpeg':
    case 'pjpeg':
      return 'jpg'
    case 'x-icon':
    case 'vnd.microsoft.icon':
      return 'ico'
    default:
      return /^[a-z0-9]+$/.test(subtype) ? subtype : 'png'
  }
}

export function dataImageInfo(dataUrl: string): { mime: string; ext: string } {
  const match = dataUrl.trim().match(DATA_IMAGE_RE)
  if (!match) {
    return { mime: 'image/png', ext: 'png' }
  }
  const mime = match[1].toLowerCase()
  return { mime, ext: mimeToIconExt(mime) }
}

function asString(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

function asNullableString(value: unknown): string | null {
  if (value == null) return null
  return typeof value === 'string' ? value : null
}

function asBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback
}

function asNumber(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function asNullableNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function asStringArray(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null
  const items = value.filter((item): item is string => typeof item === 'string')
  return items.length > 0 ? items : null
}

export function sourceToPackEntry(
  source: BrewSource,
  iconFile: string | null,
  iconUrl: string | null,
): BrewpackSource {
  return {
    url: source.url,
    name: source.name,
    category: source.category,
    icon_file: iconFile,
    icon_url: iconUrl,
    source_type: source.source_type,
    feed_type: source.feed_type,
    theme_color: source.theme_color,
    update_interval: source.update_interval,
    card_size: source.card_size,
    rsshub_route: source.rsshub_route,
    ai_style_tags: source.ai_style_tags,
    admin_only: source.admin_only,
    description: source.description,
    site_url: source.site_url,
    enabled: source.enabled,
    sort_order: source.sort_order,
  }
}

export function categoryToPackEntry(category: BrewCategory): BrewpackCategory {
  return {
    name: category.name,
    icon: category.icon,
    color: category.color,
    sort_order: category.sort_order,
  }
}

export function rsshubInstanceToPackEntry(
  instance: Pick<RsshubInstance, 'name' | 'url' | 'priority' | 'enabled'>,
): BrewpackRsshubInstance {
  return {
    name: instance.name,
    url: instance.url,
    priority: instance.priority,
    enabled: instance.enabled,
  }
}

export function buildBrewpackManifest(input: {
  sources: BrewpackSource[]
  categories: BrewpackCategory[]
  rsshubInstances: BrewpackRsshubInstance[]
  exportedAt?: string
}): BrewExportManifest {
  return {
    version: BREWPACK_VERSION,
    exported_at: input.exportedAt ?? new Date().toISOString(),
    sources: input.sources,
    categories: input.categories,
    rsshub_instances: input.rsshubInstances,
  }
}

function parsePackSource(raw: unknown): BrewpackSource | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const entry = raw as Record<string, unknown>
  const url = asString(entry.url)?.trim()
  const name = asString(entry.name)?.trim()
  if (!url || !name) return null
  return {
    url,
    name,
    category: asNullableString(entry.category),
    icon_file: asNullableString(entry.icon_file),
    icon_url: asNullableString(entry.icon_url),
    source_type: (asString(entry.source_type) as SourceType) || 'rss',
    feed_type: (asString(entry.feed_type) as BrewpackSource['feed_type']) || 'rss',
    theme_color: asNullableString(entry.theme_color),
    update_interval: asNumber(entry.update_interval, 30),
    card_size: asNullableString(entry.card_size),
    rsshub_route: asNullableString(entry.rsshub_route),
    ai_style_tags: asStringArray(entry.ai_style_tags),
    admin_only: asBoolean(entry.admin_only, false),
    description: asNullableString(entry.description),
    site_url: asNullableString(entry.site_url),
    enabled: asBoolean(entry.enabled, true),
    sort_order: asNullableNumber(entry.sort_order),
  }
}

function parsePackCategory(raw: unknown): BrewpackCategory | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const entry = raw as Record<string, unknown>
  const name = asString(entry.name)?.trim()
  if (!name) return null
  return {
    name,
    icon: asNullableString(entry.icon),
    color: asNullableString(entry.color),
    sort_order: asNumber(entry.sort_order, 0),
  }
}

function parsePackRsshubInstance(raw: unknown): BrewpackRsshubInstance | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const entry = raw as Record<string, unknown>
  const name = asString(entry.name)?.trim()
  const url = asString(entry.url)?.trim()
  if (!name || !url) return null
  return {
    name,
    url,
    priority: asNumber(entry.priority, 100),
    enabled: asBoolean(entry.enabled, true),
  }
}

export function parseBrewpackManifest(data: unknown): BrewExportManifest {
  if (!data || typeof data !== 'object' || Array.isArray(data)) {
    throw new Error('invalid_brewpack')
  }
  const raw = data as Record<string, unknown>
  const version = asString(raw.version) ?? ''
  if (!isSupportedBrewpackVersion(version) || !Array.isArray(raw.sources)) {
    throw new Error('invalid_brewpack')
  }
  const sources = raw.sources
    .map(parsePackSource)
    .filter((entry): entry is BrewpackSource => entry !== null)
  if (sources.length === 0 && raw.sources.length > 0) {
    throw new Error('invalid_brewpack')
  }
  return {
    version,
    exported_at: asString(raw.exported_at) ?? '',
    sources,
    categories: Array.isArray(raw.categories)
      ? raw.categories
          .map(parsePackCategory)
          .filter((entry): entry is BrewpackCategory => entry !== null)
      : [],
    rsshub_instances: Array.isArray(raw.rsshub_instances)
      ? raw.rsshub_instances
          .map(parsePackRsshubInstance)
          .filter((entry): entry is BrewpackRsshubInstance => entry !== null)
      : [],
  }
}

export function resolvePackIcon(
  source: BrewpackSource,
  zipIcon?: string,
): string | undefined {
  return zipIcon || source.icon_url || undefined
}

function addableSourceType(sourceType: SourceType): AddSourceRequest['source_type'] {
  if (sourceType === 'link' || sourceType === 'brewlia' || sourceType === 'rss') {
    return sourceType
  }
  return 'rss'
}

export function sourceAddPayload(
  source: BrewpackSource,
  icon?: string,
): AddSourceRequest {
  return {
    url: source.url,
    name: source.name,
    category: source.category || undefined,
    icon,
    source_type: addableSourceType(source.source_type),
    feed_type: source.feed_type,
    update_interval: source.update_interval,
    rsshub_route: source.rsshub_route || undefined,
    admin_only: source.admin_only,
    description: source.description || undefined,
    site_url: source.site_url || undefined,
    enabled: source.enabled,
    sort_order: source.sort_order ?? undefined,
  }
}

export function sourceUpdatePayload(
  source: BrewpackSource,
  icon?: string,
): UpdateSourceRequest {
  return {
    theme_color: source.theme_color || undefined,
    card_size: (source.card_size as CardSize) || undefined,
    ai_style_tags: source.ai_style_tags || undefined,
    icon,
    description: source.description || undefined,
    site_url: source.site_url || undefined,
    enabled: source.enabled,
    sort_order: source.sort_order ?? undefined,
    source_type: addableSourceType(source.source_type),
    feed_type: source.feed_type !== 'rss' ? source.feed_type : undefined,
    admin_only: source.admin_only,
    update_interval: source.update_interval,
    category: source.category || undefined,
  }
}
