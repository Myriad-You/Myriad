/**
 * Pure helpers for the host Tapp Store UI (version, source URL, progress labels).
 */

import type { RemoteStoreSource } from '../services/RemoteStoreService'

/** 比较版本号：返回 1 表示前者较新，-1 表示后者较新，0 表示相等。 */
export function compareVersions(left: string, right: string): number {
  const parts1 = left.split('.').map((n) => Number.parseInt(n, 10) || 0)
  const parts2 = right.split('.').map((n) => Number.parseInt(n, 10) || 0)
  const maxLen = Math.max(parts1.length, parts2.length)

  for (let i = 0; i < maxLen; i++) {
    const p1 = parts1[i] || 0
    const p2 = parts2[i] || 0
    if (p1 > p2) return 1
    if (p1 < p2) return -1
  }
  return 0
}

/** Normalize store source URLs so trailing slash / encoding differences still match. */
export function normalizeStoreSourceUrl(url: string): string {
  const raw = (url || '').trim()
  if (!raw) return ''
  try {
    const u = new URL(raw)
    u.hash = ''
    if (u.pathname.length > 1 && u.pathname.endsWith('/')) {
      u.pathname = u.pathname.replace(/\/+$/, '')
    }
    return u.href
  } catch {
    return raw.replace(/\/+$/, '')
  }
}

export function findStoreSource(
  sources: RemoteStoreSource[],
  sourceUrl: string,
): RemoteStoreSource | undefined {
  const target = normalizeStoreSourceUrl(sourceUrl)
  return (
    sources.find((s) => s.url === sourceUrl) ||
    sources.find((s) => normalizeStoreSourceUrl(s.url) === target)
  )
}

/** 字节数格式化为可读大小 */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB'] as const
  let value = bytes
  let unit = -1
  do {
    value /= 1024
    unit++
  } while (value >= 1024 && unit < units.length - 1)
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`
}

interface StoreProgressStrings {
  updatePreparing: string
  installPreparing: string
  updateRegistering: string
  installRegistering: string
  updateDownloading: string
  installDownloading: string
}

/** Map progress phase key → localized label (install vs update). */
export function packageProgressLabel(
  tapp: StoreProgressStrings,
  mode: 'install' | 'update',
  phase: string | null | undefined,
  percent: number,
  detail?: string | null,
): string {
  const p = (phase || 'download').toLowerCase()
  const isUpdate = mode === 'update'
  let template: string
  if (p === 'prepare') {
    template = isUpdate ? tapp.updatePreparing : tapp.installPreparing
  } else if (
    p === 'register' ||
    p === 'install' ||
    p === 'done' ||
    // Backend dual-path tags used to fall through as "Downloading…"
    p === 'server' ||
    p === 'apply'
  ) {
    template = isUpdate ? tapp.updateRegistering : tapp.installRegistering
  } else {
    template = isUpdate ? tapp.updateDownloading : tapp.installDownloading
  }
  let label = template.replace('{percent}', String(percent))
  if (detail && (p === 'download' || p === 'fetch' || p === 'client')) {
    const short =
      detail.length > 28
        ? `${detail.slice(0, 12)}…${detail.slice(-10)}`
        : detail
    label = `${label} · ${short}`
  }
  return label
}
