export interface CoSpeechExpressionOffset {
  brow: number
  eyeOpen: number
  angleY: number
}

const SHARED_OUTPUT: CoSpeechExpressionOffset = {
  brow: 0,
  eyeOpen: 0,
  angleY: 0,
}

/**
 * Translate visual-prosody envelopes into small additive rig offsets.
 *
 * This layer deliberately does not own the base expression: semantic mood,
 * workbench sliders, blinking, and gaze remain free to provide their own pose.
 * The returned object is reused because this runs once per animation frame.
 */
export function coSpeechExpressionOffset(
  phraseActivity: number,
  browAccent: number,
  headAccent: number,
): Readonly<CoSpeechExpressionOffset> {
  return writeOffset(SHARED_OUTPUT, phraseActivity, browAccent, headAccent)
}

/** Keeps authored audio/viseme input on the same visual-prosody path. */
export class CoSpeechExpressionController {
  private readonly output: CoSpeechExpressionOffset = {
    brow: 0,
    eyeOpen: 0,
    angleY: 0,
  }

  private previousEnergy = 0
  private accentStartedAt = Number.NEGATIVE_INFINITY
  private nextAccentAt = 0

  sample(
    timeSeconds: number,
    active: boolean,
    authoredEnergy: number | null,
    phraseActivity: number,
    browAccent: number,
    headAccent: number,
  ): Readonly<CoSpeechExpressionOffset> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    const energy = authoredEnergy == null ? 0 : unitInterval(authoredEnergy)
    if (
      active &&
      authoredEnergy != null &&
      energy >= 0.58 &&
      this.previousEnergy < 0.46 &&
      now >= this.nextAccentAt
    ) {
      this.accentStartedAt = now
      this.nextAccentAt = now + 0.48
    }

    if (active && authoredEnergy != null) {
      this.previousEnergy = energy
    } else {
      this.previousEnergy = 0
      this.accentStartedAt = Number.NEGATIVE_INFINITY
      this.nextAccentAt = now
    }

    const authoredActivity =
      active && authoredEnergy != null
        ? 0.22 + 0.78 * smootherstep((energy - 0.05) / 0.55)
        : 0
    const elapsed = now - this.accentStartedAt
    const authoredBrow = active ? attackReleasePulse(elapsed, 0, 0.065, 0.2) : 0
    const authoredHead = active
      ? attackReleasePulse(elapsed, 0.045, 0.1, 0.22)
      : 0
    return writeOffset(
      this.output,
      Math.max(phraseActivity, authoredActivity),
      Math.max(browAccent, authoredBrow),
      Math.max(headAccent, authoredHead),
    )
  }
}

function writeOffset(
  output: CoSpeechExpressionOffset,
  phraseActivity: number,
  browAccent: number,
  headAccent: number,
): Readonly<CoSpeechExpressionOffset> {
  const activity = unitInterval(phraseActivity)
  const browBeat = unitInterval(browAccent)
  const headBeat = unitInterval(headAccent)
  output.brow = 0.025 * activity + 0.07 * browBeat
  output.eyeOpen = -0.018 * activity + 0.014 * browBeat
  output.angleY = 0.035 * headBeat
  return output
}

function unitInterval(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}

function attackReleasePulse(
  elapsed: number,
  delay: number,
  attack: number,
  release: number,
): number {
  const shifted = elapsed - delay
  if (!Number.isFinite(shifted) || shifted < 0) return 0
  if (shifted < attack) return smootherstep(shifted / attack)
  if (shifted < attack + release) {
    return 1 - smootherstep((shifted - attack) / release)
  }
  return 0
}

function smootherstep(value: number): number {
  const bounded = unitInterval(value)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}
