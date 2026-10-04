/**
 * Two arms are not one mirrored bar. A gesture is made by one arm, the side
 * the head turns to, while the other comes along later and less; a raised
 * forearm does part of a lift at the elbow instead of the whole arm turning
 * stiff from the shoulder; and a standing figure's hip, moved out over the
 * leg that takes the weight, nudges the arm hanging beside it.
 */

export type ArmSide = 'L' | 'R'

export interface ArmIntent {
  /** `armY` after composition: a shared lift, positive away from the body. */
  open: number
  /** `armPos` after composition. */
  sway: number
  /** `angleX` after composition: positive turns the head towards image right. */
  headTurn: number
  /**
   * A standing figure's weight, +1 with the hips over the leg on the right of
   * the picture; null for a bust.
   */
  weight: number | null
  /** False freezes dynamics: each arm sits exactly on its share. */
  dynamic: boolean
}

/** What one arm is asked to do this frame. */
export interface ArmSideIntent {
  /** Lift for its shoulder, in `armY` units. */
  open: number
  /** Extra turn of its forearm about the elbow, away from the body, in radians. */
  bend: number
}

/**
 * Raise/swing are already weighted and response-filtered by the composer.
 * Shares only suppress automatic recruitment; multiplying the directed
 * contribution again would weaken attacks and prematurely erase releases.
 */
export function resolveDirectedArmIntent(
  automatic: Readonly<ArmSideIntent>,
  automaticSway: number,
  raise: number,
  swing: number,
  raiseShare: number,
  swingShare: number,
  elbow: boolean,
): ArmSideIntent & { sway: number } {
  const directedOpen = raise * (elbow ? 1 - ELBOW_SHARE : 1)
  const directedBend = elbow ? raise * ELBOW_FLEX : 0
  return {
    open: automatic.open * (1 - raiseShare) + directedOpen,
    bend: automatic.bend * (1 - raiseShare) + directedBend,
    sway: automaticSway * (1 - swingShare) + swing,
  }
}

/** A lift this large is a gesture with a leading arm; below the release it is over. */
const GESTURE_START = 0.08
const GESTURE_RELEASE = 0.03
/** A head turned this far picks the leading arm; less is looking ahead. */
const HEAD_LEAD = 0.05
/** Sway this large picks the arm it swings towards when the head is ahead. */
const SWAY_LEAD = 0.1
/** The other arm joins in with this share of the lift, this many seconds behind. */
export const FOLLOW_SHARE = 0.4
const FOLLOW_SECONDS = 0.25
/**
 * With an elbow, this share of the lift is taken off the shoulder and given
 * to the forearm, which turns out this far (radians) at a full lift.
 */
export const ELBOW_SHARE = 0.4
export const ELBOW_FLEX = 0.3
/** The hip under the weight pushes its arm out by this much of a full lift. */
export const WEIGHT_OPEN = 0.2

export class ArmChoreography {
  readonly L: ArmSideIntent = { open: 0, bend: 0 }
  readonly R: ArmSideIntent = { open: 0, bend: 0 }
  private lead: ArmSide | null = null
  /** The arm that trails; it keeps trailing after a gesture until the next one picks. */
  private follower: ArmSide = 'L'
  private followed = 0
  private initialized = false
  /** With nothing to choose by, gestures alternate arms. */
  private nextLead: ArmSide = 'R'

  step(intent: Readonly<ArmIntent>, elbows: Readonly<Record<ArmSide, boolean>>, dt: number): void {
    const open = finite(intent.open)
    const size = Math.abs(open)
    if (this.lead === null && size >= GESTURE_START) {
      this.lead = this.chooseLead(intent)
      this.follower = this.lead === 'L' ? 'R' : 'L'
    } else if (this.lead !== null && size < GESTURE_RELEASE) {
      this.lead = null
    }
    const followTarget = this.lead === null ? open : open * FOLLOW_SHARE
    const step = Number.isFinite(dt) ? Math.max(0, dt) : 0
    if (!this.initialized || !intent.dynamic) {
      this.initialized = true
      this.followed = followTarget
    } else {
      this.followed += (followTarget - this.followed) * (1 - Math.exp(-step / FOLLOW_SECONDS))
    }
    const weight = intent.weight === null ? 0 : Math.max(-1, Math.min(1, finite(intent.weight)))
    for (const side of ['L', 'R'] as const) {
      const lift = side === this.follower ? this.followed : open
      const hip = WEIGHT_OPEN * Math.max(0, side === 'R' ? weight : -weight)
      const out = this[side]
      if (elbows[side]) {
        out.open = lift * (1 - ELBOW_SHARE) + hip
        out.bend = lift * ELBOW_FLEX
      } else {
        out.open = lift + hip
        out.bend = 0
      }
    }
  }

  private chooseLead(intent: Readonly<ArmIntent>): ArmSide {
    const head = finite(intent.headTurn)
    const sway = finite(intent.sway)
    const lead: ArmSide =
      Math.abs(head) >= HEAD_LEAD
        ? head > 0 ? 'R' : 'L'
        : Math.abs(sway) >= SWAY_LEAD
          ? sway > 0 ? 'R' : 'L'
          : this.nextLead
    this.nextLead = lead === 'L' ? 'R' : 'L'
    return lead
  }
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0
}
