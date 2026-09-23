/**
 * What a settings restore could not apply as given, shown after the reload that
 * follows it (the page reloads right away, so the notice rides in
 * sessionStorage):
 * - `unresolved_media`: local media the backup cites that does not exist on
 *   this instance; the value is kept but left unbound.
 * - `preview.invalid_keys`: settings the restore skipped as invalid (e.g. a URL
 *   failing the policy saving it enforces); their current values were kept.
 *   Only key names are reported, never values.
 */

export interface UnresolvedRestoredMedia {
  /** Configuration key, e.g. `ui_wallpaper_url` or `dashboard_layout`. */
  setting: string
  url: string
}

export interface RestoreNotice {
  unresolvedMedia: UnresolvedRestoredMedia[]
  skippedSettings: string[]
}

export interface RestoreNoticeCopy {
  mediaTitle: string
  skippedTitle: string
  more: string
  wallpaper: string
  sticker: string
}

export interface ListSection {
  title: string
  lines: string[]
  more: string | null
}

export interface FormattedRestoreNotice {
  media: ListSection | null
  skipped: ListSection | null
}

type Format = (template: string, params: Record<string, string | number>) => string

export const RESTORE_NOTICE_KEY = 'myriad:settings-restore-notice'
export const RESTORE_NOTICE_LIMIT = 5

export const EMPTY_RESTORE_NOTICE: RestoreNotice = {
  unresolvedMedia: [],
  skippedSettings: [],
}

/** Tolerant parse of the server field; anything malformed is dropped. */
export function parseUnresolvedRestoredMedia(
  value: unknown,
): UnresolvedRestoredMedia[] {
  if (!Array.isArray(value)) return []
  const items: UnresolvedRestoredMedia[] = []
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue
    const { setting, url } = entry as Record<string, unknown>
    if (typeof setting !== 'string' || typeof url !== 'string' || !url) {
      continue
    }
    items.push({ setting, url })
  }
  return items
}

export function parseSettingKeys(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  return value.filter(
    (key): key is string => typeof key === 'string' && key.length > 0,
  )
}

export function hasRestoreNotice(notice: RestoreNotice): boolean {
  return notice.unresolvedMedia.length > 0 || notice.skippedSettings.length > 0
}

/** Keys shown inline (first `limit`) and how many more there are. */
export function summarizeKeys(
  keys: string[],
  limit = RESTORE_NOTICE_LIMIT,
): { shown: string[]; hidden: number } {
  const shown = keys.slice(0, Math.max(0, limit))
  return { shown, hidden: keys.length - shown.length }
}

function settingLabel(setting: string, copy: RestoreNoticeCopy): string {
  if (setting === 'ui_wallpaper_url') return copy.wallpaper
  if (setting === 'dashboard_layout') return copy.sticker
  return setting
}

function listSection(
  title: string,
  lines: string[],
  copy: RestoreNoticeCopy,
  format: Format,
  limit: number,
): ListSection | null {
  if (lines.length === 0) return null
  const { shown, hidden } = summarizeKeys(lines, limit)
  return {
    title: format(title, { count: lines.length }),
    lines: shown,
    more: hidden > 0 ? format(copy.more, { count: hidden }) : null,
  }
}

export function formatRestoreNotice(
  notice: RestoreNotice,
  copy: RestoreNoticeCopy,
  format: Format,
  limit = RESTORE_NOTICE_LIMIT,
): FormattedRestoreNotice {
  return {
    media: listSection(
      copy.mediaTitle,
      notice.unresolvedMedia.map(
        (item) => `${settingLabel(item.setting, copy)}: ${item.url}`,
      ),
      copy,
      format,
      limit,
    ),
    skipped: listSection(
      copy.skippedTitle,
      notice.skippedSettings,
      copy,
      format,
      limit,
    ),
  }
}

type NoticeStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

function defaultStorage(): NoticeStorage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage
  } catch {
    return null
  }
}

/**
 * Keep the notice for the page that loads after the restore reload. An empty
 * notice clears one left by an earlier restore.
 */
export function stashRestoreNotice(
  notice: RestoreNotice,
  storage: NoticeStorage | null = defaultStorage(),
): boolean {
  if (!storage) return false
  try {
    if (hasRestoreNotice(notice)) {
      storage.setItem(RESTORE_NOTICE_KEY, JSON.stringify(notice))
    } else {
      storage.removeItem(RESTORE_NOTICE_KEY)
    }
    return true
  } catch {
    return false
  }
}

export function readRestoreNotice(
  storage: NoticeStorage | null = defaultStorage(),
): RestoreNotice {
  if (!storage) return EMPTY_RESTORE_NOTICE
  try {
    const raw = storage.getItem(RESTORE_NOTICE_KEY)
    if (!raw) return EMPTY_RESTORE_NOTICE
    const parsed = JSON.parse(raw) as Record<string, unknown> | null
    return {
      unresolvedMedia: parseUnresolvedRestoredMedia(parsed?.unresolvedMedia),
      skippedSettings: parseSettingKeys(parsed?.skippedSettings),
    }
  } catch {
    return EMPTY_RESTORE_NOTICE
  }
}

export function clearRestoreNotice(
  storage: NoticeStorage | null = defaultStorage(),
): void {
  try {
    storage?.removeItem(RESTORE_NOTICE_KEY)
  } catch {
    // Storage unavailable: nothing was stored either.
  }
}
