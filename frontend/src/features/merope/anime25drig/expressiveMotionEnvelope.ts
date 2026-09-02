interface SpeechExpressionOffset {
  brow: number
  eyeOpen: number
  angleY: number
}

/**
 * Extra range reserved for active expression. Pose values join the compositor;
 * eye openness stays a non-pose expression response.
 */
const SEMANTIC_VERTICAL_EXTRA = 0.1
const SEMANTIC_ROLL_EXTRA = 0.18
const SPEECH_HEAD_EXTRA = 0.15
const SPEECH_FACE_EXTRA = 0.12

export function semanticVerticalMotionOffset(value: number): number {
  return finiteOrZero(value) * SEMANTIC_VERTICAL_EXTRA
}

export function semanticRollMotionOffset(value: number): number {
  return finiteOrZero(value) * SEMANTIC_ROLL_EXTRA
}

export function speechHeadMotionOffset(value: number): number {
  return finiteOrZero(value) * SPEECH_HEAD_EXTRA
}

export function speechBrowMotionOffset(value: number): number {
  return finiteOrZero(value) * SPEECH_FACE_EXTRA
}

export function expressiveEyeOpenOffset(
  speech: Readonly<SpeechExpressionOffset>,
): number {
  return finiteOrZero(speech.eyeOpen) * SPEECH_FACE_EXTRA
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? value : 0
}
