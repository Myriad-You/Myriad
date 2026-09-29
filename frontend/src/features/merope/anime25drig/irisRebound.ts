import type { JellyTuning } from './jellyVolume'
import { JellyVolume } from './jellyVolume'

/**
 * The iris is a soft, wet thing, not a sticker: when the lid snaps open or
 * the gaze darts, it squashes and stretches a little and wobbles back, as
 * Live2D riggers make eyes プルプル with a physics output driven by the lids.
 * Both are volume, not travel: the iris stays where the gaze puts it and
 * keeps its area, it only changes shape.
 */

/** A small, taut body: quick and lively, settled within a third of a second. */
const LID_JELLY: Readonly<JellyTuning> = {
  hz: 6,
  damping: 0.24,
  gain: 0.65,
  stretchLimit: 0.05,
  swayLimit: 0,
}

const GAZE_JELLY: Readonly<JellyTuning> = {
  hz: 6.5,
  damping: 0.26,
  gain: 0.16,
  stretchLimit: 0.035,
  swayLimit: 0,
}

/** A wide eye opens further than a plain one; the iris feels that too. */
const WIDE_TRAVEL = 0.3
/**
 * Below this the wobble is gone to the eye; report the iris as still, or
 * the eye's cached geometry would be rebuilt every frame for nothing.
 */
const REST = 2e-4

export class Anime25DIrisRebound {
  /** Horizontal and vertical scale of the iris about its centre. */
  x = 1
  y = 1
  private readonly lid = new JellyVolume(LID_JELLY)
  private readonly gazeX = new JellyVolume(GAZE_JELLY)
  private readonly gazeY = new JellyVolume(GAZE_JELLY)

  /**
   * `eyeOpen` is how open the eye is drawn now, `eyeX/Y` where it looks.
   * `suppressed` holds the iris still under effects that redraw the eye.
   */
  step(
    elapsed: number,
    eyeOpen: number,
    eyeWide: number,
    eyeX: number,
    eyeY: number,
    suppressed: boolean,
  ): void {
    const lid = eyeOpen + WIDE_TRAVEL * eyeWide
    this.lid.step(0, lid, 0, 1, 1, elapsed, !suppressed)
    this.gazeX.step(eyeX, 0, 1, 0, 1, elapsed, !suppressed)
    this.gazeY.step(0, eyeY, 0, 1, 1, elapsed, !suppressed)
    // Each stretch keeps the iris's area: longer one way, narrower the other.
    const vertical = 1 + this.lid.stretch + this.gazeY.stretch
    const horizontal = 1 + this.gazeX.stretch
    this.x = horizontal / vertical
    this.y = vertical / horizontal
    if (Math.abs(this.x - 1) < REST && Math.abs(this.y - 1) < REST) {
      this.x = 1
      this.y = 1
    }
  }
}
