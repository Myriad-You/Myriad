import type { MoodBand } from '../../../components/agent/meropeVitals'
import type {
  PerformanceBaseline,
  PerformanceDirective,
} from '../../../services/agent/types'
import { moodBand } from '../../../components/agent/meropeVitals'

export interface RigBearing extends PerformanceBaseline {
  revision: number
}

/** Same mapping as `motion_local::baseline_expression` without the valence step. */
const STANDING_FROM_BAND: Record<MoodBand, Omit<RigBearing, 'revision'>> = {
  floor: {
    expression: 'withdrawn',
    posture: 'closed',
    motionEnergy: 0.9 * 0.85,
    attention: 0.4,
  },
  sad: {
    expression: 'subdued',
    posture: 'neutral',
    motionEnergy: 0.9 * 0.92,
    attention: 0.4,
  },
  tense: {
    expression: 'tense',
    posture: 'neutral',
    motionEnergy: 0.9 * 1.05,
    attention: 0.4,
  },
  calm: {
    expression: 'steady',
    posture: 'neutral',
    motionEnergy: 0.9,
    attention: 0.4,
  },
  excited: {
    expression: 'warm',
    posture: 'open',
    motionEnergy: 0.9 * 1.12,
    attention: 0.4,
  },
}

export function standingBearingFromAffect(
  mood: number,
  arousal: number,
): RigBearing {
  return {
    ...STANDING_FROM_BAND[moodBand(mood, arousal)],
    revision: 0,
  }
}

export function bearingFromDirective(
  directive: PerformanceDirective,
): RigBearing | null {
  const baseline = directive.plan.baseline
  if (!baseline) return null
  return {
    ...baseline,
    revision: Math.max(0, Math.trunc(directive.moodRevision)),
  }
}
