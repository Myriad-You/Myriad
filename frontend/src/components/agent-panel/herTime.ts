import type { MeropeDoingResponse } from '../../services/agent/types'

export type HerDoing = NonNullable<MeropeDoingResponse['doing']>
export type HerLazing = NonNullable<MeropeDoingResponse['lazing']>

/** Look again this long after her current thing ends, or this often at most. */
const AFTER_END_MS = 5_000
const AT_MOST_MS = 60_000
/** When she cannot be asked (off, or no access), ask again much later. */
export const UNAVAILABLE_RETRY_MS = 5 * 60_000

/** When to ask again what she is doing. */
export function nextLookMs(response: MeropeDoingResponse): number {
  const current = response.doing ?? response.lazing
  if (!current) return AT_MOST_MS
  const left =
    Date.parse(current.ends) - Date.parse(response.now) + AFTER_END_MS
  if (!Number.isFinite(left)) return AT_MOST_MS
  return Math.min(AT_MOST_MS, Math.max(AFTER_END_MS, left))
}

/** Whether this player is on the song she is listening to, and playing. */
export function listeningAlong(
  doing: HerDoing | null,
  player: Record<string, unknown> | null,
): boolean {
  if (doing?.thing.kind !== 'song' || !player) return false
  const current = player.currentSong as { id?: unknown } | null | undefined
  return player.isPlaying === true && String(current?.id) === doing.thing.id
}

/** The title to show for what she is doing. */
export function herTitle(doing: HerDoing): string {
  return doing.thing.kind === 'song' ? doing.thing.name : doing.thing.title
}
