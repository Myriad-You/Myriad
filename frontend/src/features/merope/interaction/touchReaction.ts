import type { TouchObservation } from './touchGesture'
import { moodBand } from '../../../components/agent/meropeVitals'

export const TOUCH_REACTIONS = ['notice', 'accept', 'hesitate', 'withdraw'] as const
export type TouchReaction = (typeof TOUCH_REACTIONS)[number]

/** A sampled response submitted to the renderer, not a requested plan. */
export interface PresentedTouchReaction {
  behaviorId: string
  reaction: TouchReaction
  atMs: number
}

/** Conservative local appraisal, not an inference about the user's intention. */
export function selectTouchReaction(
  touch: TouchObservation,
  mood: number,
  arousal: number,
): TouchReaction {
  const band = moodBand(mood, arousal)
  if (touch.repeatCount >= 3 && touch.region === 'face') return 'withdraw'
  if (touch.gesture === 'contact' && touch.repeatCount === 0) return 'notice'
  if (band === 'tense' || band === 'floor') return 'hesitate'
  if (touch.region === 'hair') {
    return band === 'sad' ? 'hesitate' : 'accept'
  }
  if (touch.region === 'face' || touch.region === 'accessory') return 'hesitate'
  if (touch.gesture === 'tap' && touch.repeatCount >= 2) return 'hesitate'
  return 'notice'
}
