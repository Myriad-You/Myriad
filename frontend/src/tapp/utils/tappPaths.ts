/**
 * Canonical Tapp SPA paths — prefer these over string literals at call sites.
 *
 * Multi-window entry (single code path = TappRunPage + WindowManager):
 * - Empty multi:    /tapp/run?multi=true          → tappRunMultiPath()
 * - Seeded multi:   /tapp/run/:id?multi=true     → tappRunPath(id, { multi: true })
 * - Store multi:    redirects store?multi → run/:hostStore?multi
 */

export const TAPP_LIST_PATH = '/tapp' as const
export const TAPP_STORE_PATH = '/tapp/store' as const
export const TAPP_PLAYGROUND_PATH = '/tapp/playground' as const

export function tappRunPath(
  tappId: string,
  opts?: { multi?: boolean },
): string {
  const base = `/tapp/run/${encodeURIComponent(tappId)}`
  return opts?.multi ? `${base}?multi=true` : base
}

/** Multi-window without a seed id: /tapp/run?multi=true */
export function tappRunMultiPath(): string {
  return '/tapp/run?multi=true'
}

export function tappDetailPath(tappId: string): string {
  return `/tapp/detail/${encodeURIComponent(tappId)}`
}
