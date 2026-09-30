import { getGlobalState } from '../../../hooks/musicPlayer/globalState'
import { playbackFailure } from '../../../utils/playbackFailures'

/**
 * What came of what she just did on this player. When she puts a song of
 * hers on (shared, or to listen together), whether it played: a friend
 * would see it did not play on your side, and would not say "let me press
 * it again" without knowing. She hears it the next time she speaks.
 */

export type HerActOutcome = 'playing' | 'failed' | 'not_started' | 'not_available'

export interface HerAct {
  act: 'share' | 'join'
  song: string
  artist: string
  outcome: HerActOutcome
  /** A music error key, when the player learned why. */
  reason?: string
  secondsAgo: number
}

interface Noted extends Omit<HerAct, 'secondsAgo'> {
  songId: string
  at: number
}

/** What she did is in mind this long. */
const REMEMBERED_MS = 30 * 60_000
/** A song not playing after this long has not started. */
const WATCH_MS = 10_000
/** After it fails, how long the reason may take to come. */
const REASON_WAIT_MS = 2_000
const STEP_MS = 250

let last: Noted | null = null

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

interface SongLike {
  id: string
  name: string
  artist: string
}

/** She meant to put `song` on, but this player cannot play it at all. */
export function noteNotAvailable(act: HerAct['act'], song: SongLike, now = Date.now()): void {
  last = { act, songId: String(song.id), song: song.name, artist: song.artist, outcome: 'not_available', at: now }
}

/**
 * She put `song` on this player: watch until it plays, fails, or is not
 * playing after a while, and keep what came of it.
 */
export async function watchHerSong(
  act: HerAct['act'],
  song: SongLike,
  clock: { now: () => number; wait: (ms: number) => Promise<void> } = { now: Date.now, wait: sleep },
): Promise<HerActOutcome> {
  const started = clock.now()
  const noted: Noted = {
    act,
    songId: String(song.id),
    song: song.name,
    artist: song.artist,
    outcome: 'not_started',
    at: started,
  }
  last = noted
  let seen = false
  while (clock.now() - started < WATCH_MS) {
    // A newer act of hers replaces this one.
    if (last !== noted) return noted.outcome
    const failure = playbackFailure(noted.songId, started)
    if (failure) {
      noted.outcome = 'failed'
      const failedAt = clock.now()
      while (!noted.reason && clock.now() - failedAt < REASON_WAIT_MS) {
        noted.reason = playbackFailure(noted.songId, started)?.reason
        if (!noted.reason) await clock.wait(STEP_MS)
      }
      return noted.outcome
    }
    const state = getGlobalState()
    const current = state?.currentSong as { id?: unknown } | null | undefined
    if (current && String(current.id) === noted.songId) {
      seen = true
      if (state?.isPlaying === true && state?.isAudioLoading !== true) {
        noted.outcome = 'playing'
        return noted.outcome
      }
    } else if (seen) {
      // The player moved on before it ever played.
      noted.outcome = 'failed'
      return noted.outcome
    }
    await clock.wait(STEP_MS)
  }
  return noted.outcome
}

/** What came of her last act here, if it was lately. */
export function herLastAct(now = Date.now()): HerAct | null {
  if (!last || now - last.at > REMEMBERED_MS) return null
  const { act, song, artist, outcome, reason } = last
  return {
    act,
    song,
    artist,
    outcome,
    ...(reason ? { reason } : {}),
    secondsAgo: Math.max(0, Math.round((now - last.at) / 1000)),
  }
}

/** For tests: forget what she did. */
export function forgetHerActs(): void {
  last = null
}
