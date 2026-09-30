/**
 * Songs this player could not play lately, and why when the proxy said:
 * the player notes them as they fail, so whoever put a song on can know
 * what came of it after the player has already moved on.
 */

/** Failures kept, at most. */
const KEPT = 32

const failures = new Map<string, { at: number; reason?: string }>()

/** `songId` would not play; `reason` is a music error key, once known. */
export function notePlaybackFailure(songId: string, reason?: string | null, now = Date.now()): void {
  const known = failures.get(songId)
  failures.delete(songId)
  failures.set(songId, {
    at: known && !reason ? known.at : now,
    reason: reason || known?.reason,
  })
  while (failures.size > KEPT) {
    const oldest = failures.keys().next().value
    if (oldest === undefined) break
    failures.delete(oldest)
  }
}

/** Whether `songId` failed at or after `since`, and why if known. */
export function playbackFailure(songId: string, since: number): { reason?: string } | null {
  const failure = failures.get(songId)
  return failure && failure.at >= since ? { reason: failure.reason } : null
}
