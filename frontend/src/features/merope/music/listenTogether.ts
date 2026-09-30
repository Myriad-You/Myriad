import type { MeropeDoingResponse, MeropeThing } from '../../../services/agent/types'
import type { MusicSource, Song } from '../../../utils/musicPlayer'
import { getGlobalState } from '../../../hooks/musicPlayer/globalState'
import { agentService } from '../../../services/agent/agentApi'
import { emitAppEvent } from '../../../utils/appEvents'
import { getNeteaseAudioUrlImmediate } from '../../../utils/musicPlayer'
import { noteNotAvailable, watchHerSong } from './herActs'

/** Too close to the end: joining would only catch the last notes. */
const NEAR_END_SECONDS = 8
/** Starting a few seconds late is still listening together. */
const SEEK_AFTER_SECONDS = 3
const WAIT_FOR_PLAYBACK_MS = 10_000

type MeropeSong = Extract<MeropeThing, { kind: 'song' }>

export interface ListenTogetherPlan {
  song: Song
  /** Where the song sits in this player's queue, when it is there. */
  index: number | null
  /** Where she is in it, in seconds, when the plan was made. */
  offsetSeconds: number
}

/**
 * How to put the song she is listening to on this player, where she is in it.
 * Null when she is not listening to anything, it is nearly over, or this
 * player cannot play it.
 */
export function planListenTogether(
  response: MeropeDoingResponse,
  playlist: readonly unknown[] | null,
  audioUrl: (id: string) => string,
  fetchedAt: number,
  now: number,
): ListenTogetherPlan | null {
  const thing = response.doing?.thing
  if (!response.doing || thing?.kind !== 'song') return null
  const elapsed =
    (Date.parse(response.now) - Date.parse(response.doing.started)) / 1000 +
    Math.max(0, now - fetchedAt) / 1000
  const duration = thing.durationMs / 1000
  if (!Number.isFinite(elapsed) || elapsed >= duration - NEAR_END_SECONDS) return null
  const placed = placeSong(thing, playlist, audioUrl)
  return placed ? { ...placed, offsetSeconds: Math.max(0, elapsed) } : null
}

/**
 * A song of hers as this player would play it: from its queue when it is
 * there, else fetched directly. Null when this player cannot fetch it.
 */
export function placeSong(
  thing: MeropeSong,
  playlist: readonly unknown[] | null,
  audioUrl: (id: string) => string,
): { song: Song; index: number | null } | null {
  const index = (playlist ?? []).findIndex(
    (entry) =>
      typeof entry === 'object' &&
      entry !== null &&
      String((entry as { id?: unknown }).id) === thing.id,
  )
  if (index < 0 && thing.source !== 'netease') return null
  const song: Song = {
    id: thing.id,
    name: thing.name,
    artist: thing.artist,
    album: thing.album,
    cover: thing.cover,
    url: index < 0 ? audioUrl(thing.id) : '',
    // The player keeps seconds.
    duration: thing.durationMs / 1000,
    source: thing.source as MusicSource,
    isVip: false,
  }
  return {
    song: index < 0 ? song : { ...song, ...(playlist?.[index] as Partial<Song>) },
    index: index < 0 ? null : index,
  }
}

function startSong(placed: { song: Song; index: number | null }): void {
  if (placed.index !== null) {
    emitAppEvent('play-song-at-index', { index: placed.index, song: placed.song })
  } else {
    emitAppEvent('play-song', { song: placed.song })
  }
}

function isPlaying(songId: string): boolean {
  const state = getGlobalState()
  const current = state?.currentSong as { id?: unknown } | null | undefined
  return (
    String(current?.id) === songId &&
    state?.isPlaying === true &&
    state?.isAudioLoading !== true
  )
}

async function untilPlaying(songId: string): Promise<boolean> {
  const deadline = Date.now() + WAIT_FOR_PLAYBACK_MS
  while (Date.now() < deadline) {
    if (isPlaying(songId)) return true
    await new Promise((resolve) => setTimeout(resolve, 250))
  }
  return false
}

/** Put the song she is listening to on this player, where she is in it. */
export async function listenTogether(): Promise<boolean> {
  const fetchedAt = Date.now()
  const response = await agentService.getDoing().catch(() => null)
  if (!response) return false
  const playlist = getGlobalState()?.playlist
  const planned = Date.now()
  const plan = planListenTogether(
    response,
    Array.isArray(playlist) ? playlist : null,
    getNeteaseAudioUrlImmediate,
    fetchedAt,
    planned,
  )
  if (!plan) return false
  startSong(plan)
  void watchHerSong('join', plan.song)
  if (!(await untilPlaying(plan.song.id))) return true
  const position = plan.offsetSeconds + (Date.now() - planned) / 1000
  if (position > SEEK_AFTER_SECONDS) {
    emitAppEvent('music-player-seek', { position })
  }
  return true
}

/** Put a song she heard and chose to share on this player, from the start. */
export function playHerSong(thing: MeropeThing | null | undefined): boolean {
  if (thing?.kind !== 'song') return false
  const playlist = getGlobalState()?.playlist
  const placed = placeSong(
    thing,
    Array.isArray(playlist) ? playlist : null,
    getNeteaseAudioUrlImmediate,
  )
  if (!placed) {
    noteNotAvailable('share', thing)
    return false
  }
  startSong(placed)
  void watchHerSong('share', placed.song)
  return true
}
