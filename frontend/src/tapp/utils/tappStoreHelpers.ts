import type { RemoteStoreSource } from '../services/RemoteStoreService'
import { formatCurrent } from '../../i18n/localeCopy'

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

export function normalizeStoreSourceUrl(url: string): string {
  const raw = (url || '').trim()
  if (!raw) return ''
  try {
    const u = new URL(raw)
    u.hash = ''
    if (u.pathname.length > 1 && u.pathname.endsWith('/')) {
      u.pathname = u.pathname.replaceAll(/\/+$/g, '')
    }
    return u.href
  } catch {
    return raw.replaceAll(/\/+$/g, '')
  }
}

export function findStoreSource(
  sources: RemoteStoreSource[],
  sourceUrl: string,
): RemoteStoreSource | undefined {
  const target = normalizeStoreSourceUrl(sourceUrl)
  return (
    sources.find((s) => s.url === sourceUrl) ??
    sources.find((s) => normalizeStoreSourceUrl(s.url) === target)
  )
}

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
    p === 'server' ||
    p === 'apply'
  ) {
    template = isUpdate ? tapp.updateRegistering : tapp.installRegistering
  } else {
    template = isUpdate ? tapp.updateDownloading : tapp.installDownloading
  }
  let label = formatCurrent(template, { percent })
  if (detail && (p === 'download' || p === 'fetch' || p === 'client')) {
    const short =
      detail.length > 28
        ? `${detail.slice(0, 12)}…${detail.slice(-10)}`
        : detail
    label = `${label} · ${short}`
  }
  return label
}
