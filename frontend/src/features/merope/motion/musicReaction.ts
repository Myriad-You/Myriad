import type { MusicMode, MusicMotionSignal } from '../singing/musicSignal'
import type { BehaviorQuality } from './behavior'

export const MUSIC_QUALITY: Readonly<Record<MusicMode, BehaviorQuality>> = {
  listen: {
    extent: 1.08,
    tempo: 0.82,
    power: 0.78,
    fluidity: 0.92,
    directness: 0.58,
    rebound: 0.3,
    asymmetry: 0.36,
    density: 0.65,
  },
  hum: {
    extent: 1.12,
    tempo: 0.84,
    power: 0.85,
    fluidity: 0.94,
    directness: 0.6,
    rebound: 0.35,
    asymmetry: 0.4,
    density: 0.72,
  },
  sing: {
    extent: 1.2,
    tempo: 0.85,
    power: 0.92,
    fluidity: 0.94,
    directness: 0.7,
    rebound: 0.3,
    asymmetry: 0.44,
    density: 0.76,
  },
  settle: {
    extent: 1,
    tempo: 0.7,
    power: 0.65,
    fluidity: 1.1,
    directness: 0.45,
    rebound: 0.1,
    asymmetry: 0.3,
    density: 0.3,
  },
}

/**
 * Sparse music participation decisions for the existing behavior scheduler.
 * Listening is not permanent singing or perpetual motion. Musical stilling:
 * Upham et al. 2024, https://doi.org/10.1177/20592043241233422
 * Humming bouts below are character art direction, not vocal detection.
 */
export class MusicReactionPlanner {
  private quietSince = Number.NaN
  private nextChoiceAt = 0
  private humming = false
  private seed = 1

  reset(trackId: string): void {
    this.quietSince = Number.NaN
    this.nextChoiceAt = 0
    this.humming = false
    this.seed = 2166136261
    for (const char of trackId)
      this.seed = Math.imul(this.seed ^ char.charCodeAt(0), 16777619) >>> 0
  }

  sample(
    signal: Readonly<MusicMotionSignal>,
    nowMs: number,
    paused: boolean,
    hasLyrics: boolean,
  ): MusicMode {
    if (paused) return 'settle'
    const silent = signal.audio !== null && signal.audio.energy < 0.025
    if (!silent) this.quietSince = Number.NaN
    else if (!Number.isFinite(this.quietSince)) this.quietSince = nowMs
    // Do not turn a gap between two drum hits into a participation change.
    if (silent && nowMs - this.quietSince >= 350) return 'settle'
    if (
      hasLyrics &&
      signal.phrase &&
      signal.sampleTimeSeconds < signal.phrase.end
    ) {
      return 'sing'
}
    if (hasLyrics || !signal.audio || signal.audio.energy < 0.12)
      return 'listen'
    if (nowMs >= this.nextChoiceAt) {
      this.humming = !this.humming
      this.seed ^= this.seed << 13
      this.seed ^= this.seed >>> 17
      this.seed ^= this.seed << 5
      const random = (this.seed >>> 0) / 4294967296
      this.nextChoiceAt =
        nowMs + (this.humming ? 3.2 + random * 4.8 : 2.4 + random * 4.2) * 1_000
    }
    return this.humming ? 'hum' : 'listen'
  }
}
