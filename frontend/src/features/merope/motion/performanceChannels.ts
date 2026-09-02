import type {
  PerformanceCue,
  PerformanceDirective,
} from '../../../services/agent/types'
import type { MotionChannel } from './channels'
import { performanceCueChannels } from '../anime25drig/performanceCueDefinitions'

export function cueOccupiesHeadBody(cue: PerformanceCue): boolean {
  return performanceCueChannels(cue.intent).includes('headBody')
}

export function cueOccupiesGaze(cue: PerformanceCue): boolean {
  return performanceCueChannels(cue.intent).includes('gaze')
}

/**
 * Channels a Lite plan actually writes. Face plans do not take the body;
 * posture and body cues do. Mouth stays speech/music owned.
 * Timed occupancy lives in performanceLeaseWindows — this is classification.
 */
export function performanceOccupiedChannels(
  directive: PerformanceDirective,
): MotionChannel[] {
  const channels = new Set<MotionChannel>()
  const baseline = directive.plan.baseline
  if (baseline && baseline.posture !== 'neutral') channels.add('headBody')
  for (const cue of directive.plan.cues) {
    for (const channel of performanceCueChannels(cue.intent)) {
      channels.add(channel)
    }
  }
  return [...channels]
}
