/**
 * Local media cited by a restored settings backup that does not exist on this
 * instance. The server keeps those values as they were and leaves them unbound
 * (`unresolved_media` in the restore response); the page reloads right after a
 * restore, so the notice is carried across the reload in sessionStorage.
 */

export interface UnresolvedRestoredMedia {
  /** Configuration key, e.g. `ui_wallpaper_url` or `dashboard_layout`. */
  setting: string
  url: string
}

export interface UnresolvedMediaCopy {
  title: string
  more: string
  wallpaper: string
  sticker: string
}

export interface UnresolvedMediaNotice {
  title: string
  lines: string[]
  more: string | null
}

type Format = (template: string, params: Record<string, string | number>) => string

export const RESTORE_MEDIA_NOTICE_KEY = 'myriad:settings-restore-unresolved-media'
export const RESTORE_MEDIA_NOTICE_LIMIT = 5

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

function settingLabel(setting: string, copy: UnresolvedMediaCopy): string {
  if (setting === 'ui_wallpaper_url') return copy.wallpaper
  if (setting === 'dashboard_layout') return copy.sticker
  return setting
}

export function formatUnresolvedMediaNotice(
  items: UnresolvedRestoredMedia[],
  copy: UnresolvedMediaCopy,
  format: Format,
  limit = RESTORE_MEDIA_NOTICE_LIMIT,
): UnresolvedMediaNotice {
  const shown = items.slice(0, Math.max(0, limit))
  const hidden = items.length - shown.length
  return {
    title: format(copy.title, { count: items.length }),
    lines: shown.map((item) => `${settingLabel(item.setting, copy)}: ${item.url}`),
    more: hidden > 0 ? format(copy.more, { count: hidden }) : null,
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

/** Keep the notice for the page that loads after the restore reload. */
export function stashRestoreMediaNotice(
  items: UnresolvedRestoredMedia[],
  storage: NoticeStorage | null = defaultStorage(),
): boolean {
  if (!storage) return false
  try {
    if (items.length === 0) {
      storage.removeItem(RESTORE_MEDIA_NOTICE_KEY)
    } else {
      storage.setItem(RESTORE_MEDIA_NOTICE_KEY, JSON.stringify(items))
    }
    return true
  } catch {
    return false
  }
}

export function readRestoreMediaNotice(
  storage: NoticeStorage | null = defaultStorage(),
): UnresolvedRestoredMedia[] {
  if (!storage) return []
  try {
    const raw = storage.getItem(RESTORE_MEDIA_NOTICE_KEY)
    return raw ? parseUnresolvedRestoredMedia(JSON.parse(raw)) : []
  } catch {
    return []
  }
}

export function clearRestoreMediaNotice(
  storage: NoticeStorage | null = defaultStorage(),
): void {
  try {
    storage?.removeItem(RESTORE_MEDIA_NOTICE_KEY)
  } catch {
    // Storage unavailable: nothing was stored either.
  }
}
