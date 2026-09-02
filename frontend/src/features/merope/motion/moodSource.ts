import type { MoodBand } from '../../../components/agent/meropeVitals'
import type { MeropeActivity } from '../types'
import type { RigBearing } from './bearing'
import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'
import type { MoodIntent } from './intents'
import { moodBand } from '../../../components/agent/meropeVitals'
import { standingBearingFromAffect } from './bearing'

const DEFAULT_MOOD = 70
const DEFAULT_AROUSAL = 48

/** Mood/activity baseline. Expression only; thinking is gated as ambient. */
export class MoodMotionSource {
  private handle: MotionLeaseHandle | null = null
  private intent: MoodIntent = {
    mood: DEFAULT_MOOD,
    arousal: DEFAULT_AROUSAL,
    activity: 'idle',
  }

  private band: MoodBand = moodBand(DEFAULT_MOOD, DEFAULT_AROUSAL)
  private bearing: RigBearing = standingBearingFromAffect(
    DEFAULT_MOOD,
    DEFAULT_AROUSAL,
  )

  constructor(
    private readonly coordinator: RigMotionCoordinator,
    private readonly onChange: (
      intent: MoodIntent,
      bandChanged: boolean,
    ) => void,
  ) {}

  current(): MoodIntent {
    return this.intent
  }

  currentBearing(): RigBearing {
    return this.bearing
  }

  set(
    mood: number,
    activity: MeropeActivity,
    arousal: number = DEFAULT_AROUSAL,
  ): void {
    const nextBand = moodBand(mood, arousal)
    const bandChanged = nextBand !== this.band
    this.intent = { mood, arousal, activity }
    this.band = nextBand
    if (bandChanged) {
      this.bearing = standingBearingFromAffect(mood, arousal)
    }
    this.handle =
      this.coordinator.renew(this.handle, ['expression']) ??
      this.coordinator.claim('mood', ['expression'])
    this.onChange(this.intent, bandChanged)
  }

  release(): void {
    this.coordinator.release(this.handle)
    this.handle = null
  }
}
