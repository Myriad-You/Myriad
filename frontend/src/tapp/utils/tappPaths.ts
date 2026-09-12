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

export function tappRunMultiPath(): string {
  return '/tapp/run?multi=true'
}

export function tappDetailPath(tappId: string): string {
  return `/tapp/detail/${encodeURIComponent(tappId)}`
}
