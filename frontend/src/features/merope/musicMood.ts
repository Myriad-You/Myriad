import { agentService } from '../../services/agent/agentApi'
import { ApiError } from '../../services/api'
import { isKnownGuest } from '../../utils/authState'
import { dispatchMeropeState } from './performanceEvents'

export const MUSIC_MOOD_REPORT_SECONDS = 10 * 60
const MUSIC_MOOD_MAX_REPORT_SECONDS = 30 * 60
const RETRY_AFTER_FAILURE_MS = 5 * 60_000
const DISABLED_RECHECK_MS = 30 * 60_000

interface PlaybackSample {
  songId: string | null
  currentTime: number | null
  playing: boolean
  audible: boolean
  loading: boolean
}

interface TimedPlaybackSample extends PlaybackSample {
  observedAtMs: number
}

export class MusicListeningAccumulator {
  private previous: TimedPlaybackSample | null = null
  private accumulatedSeconds = 0

  observe(sample: PlaybackSample, observedAtMs: number): void {
    const previous = this.previous
    if (
      previous &&
      previous.playing &&
      previous.audible &&
      !previous.loading &&
      sample.playing &&
      sample.audible &&
      !sample.loading &&
      sample.songId !== null &&
      sample.songId === previous.songId &&
      sample.currentTime !== null &&
      previous.currentTime !== null
    ) {
      const mediaDelta = sample.currentTime - previous.currentTime
      const wallDelta = (observedAtMs - previous.observedAtMs) / 1000
      if (mediaDelta > 0 && wallDelta > 0) {
        const credibleDelta = Math.min(mediaDelta, wallDelta * 1.25 + 1)
        this.accumulatedSeconds = Math.min(
          MUSIC_MOOD_MAX_REPORT_SECONDS,
          this.accumulatedSeconds + credibleDelta,
        )
      }
    }
    this.previous = { ...sample, observedAtMs }
  }

  qualified(): boolean {
    return this.accumulatedSeconds >= MUSIC_MOOD_REPORT_SECONDS
  }

  takeReport(): number {
    if (!this.qualified()) return 0
    const seconds = Math.floor(this.accumulatedSeconds)
    this.accumulatedSeconds = 0
    return seconds
  }

  restore(seconds: number): void {
    this.accumulatedSeconds = Math.min(
      MUSIC_MOOD_MAX_REPORT_SECONDS,
      this.accumulatedSeconds + Math.max(0, seconds),
    )
  }

  reset(): void {
    this.previous = null
    this.accumulatedSeconds = 0
  }

  secondsForTest(): number {
    return this.accumulatedSeconds
  }
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object'
    ? (value as Record<string, unknown>)
    : null
}

function finiteNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function playbackSample(snapshot: Record<string, unknown>): PlaybackSample {
  const song = record(snapshot.currentSong)
  const rawId = song?.id
  const songId =
    typeof rawId === 'string' || typeof rawId === 'number'
      ? String(rawId)
      : null
  const volume = finiteNumber(snapshot.volume)
  return {
    songId,
    currentTime: finiteNumber(snapshot.currentTime),
    playing: snapshot.isPlaying === true,
    audible: volume === null || volume > 0.001,
    loading: snapshot.isAudioLoading === true,
  }
}

export function bindMusicMoodListening(): () => void {
  const accumulator = new MusicListeningAccumulator()
  let snapshot: Record<string, unknown> = {
    ...((window as { __musicPlayerState?: Record<string, unknown> })
      .__musicPlayerState ?? {}),
  }
  let requestInFlight = false
  let serverQuietUntilMs = 0
  let active = true

  const maybeReport = () => {
    if (
      requestInFlight ||
      !accumulator.qualified() ||
      Date.now() < serverQuietUntilMs
    ) {
      return
    }
    const listenedSeconds = accumulator.takeReport()
    if (isKnownGuest()) {
      accumulator.reset()
      return
    }
    requestInFlight = true
    void agentService
      .creditMusicListening(listenedSeconds)
      .then((result) => {
        serverQuietUntilMs =
          Date.now() + Math.max(0, result.nextCreditInSeconds) * 1000
        if (active && result.credited) {
          dispatchMeropeState({
            mood: result.mood,
            activity: result.activity,
          })
        }
      })
      .catch((error: unknown) => {
        if (
          error instanceof ApiError &&
          (error.code === 'merope_disabled' || error.code === 'login_required')
        ) {
          accumulator.reset()
          serverQuietUntilMs = Date.now() + DISABLED_RECHECK_MS
        } else {
          accumulator.restore(listenedSeconds)
          serverQuietUntilMs = Date.now() + RETRY_AFTER_FAILURE_MS
        }
      })
      .finally(() => {
        requestInFlight = false
      })
  }

  const observe = () => {
    accumulator.observe(playbackSample(snapshot), Date.now())
    maybeReport()
  }
  const onState = (event: Event) => {
    const detail = record((event as CustomEvent<unknown>).detail)
    if (!detail) return
    snapshot = { ...snapshot, ...detail }
    observe()
  }
  const onProgress = (event: Event) => {
    const detail = record((event as CustomEvent<unknown>).detail)
    if (!detail) return
    const currentSong = record(snapshot.currentSong)
    const currentSongId = currentSong?.id
    const progressSongId = detail.songId
    if (
      progressSongId != null &&
      currentSongId != null &&
      String(progressSongId) !== String(currentSongId)
    ) {
      return
    }
    snapshot = { ...snapshot, ...detail }
    observe()
  }

  window.addEventListener('music-player-state-change', onState)
  window.addEventListener('music-player-progress', onProgress)
  observe()
  return () => {
    active = false
    window.removeEventListener('music-player-state-change', onState)
    window.removeEventListener('music-player-progress', onProgress)
  }
}
