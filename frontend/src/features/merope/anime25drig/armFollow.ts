/**
 * The sleeves are the one body segment with no path from torso motion. The
 * torso shell carries the garment, the chest spring carries the bust, and the
 * arms were left to whatever a cue happened to author — so every large body
 * move that is not `shoulderEase` turned the torso and left the arms dead.
 *
 * Arm swing during trunk motion is largely passive: the arm behaves as a mass
 * damper powered by trunk movement rather than being driven at the shoulder
 * (Pontzer et al. 2009, J Exp Biol 212:523), and the distal segment lags the
 * proximal one before being carried along (Putnam 1993, J Biomech 26:125). So
 * the sleeve's answer to the torso is derived here from the torso's own
 * rotation instead of being re-authored on every cue.
 */

/**
 * Free-swing natural frequency of the human arm is about 1 Hz — far below the
 * 8 Hz the authored arm channels are filtered at. The follower runs at the
 * arm's own bandwidth, so a fast torso turn genuinely leaves it behind.
 */
export const ARM_FOLLOW_HZ = 1.1

/** Torso yaw the sleeve has not caught up with that counts as a full swing. */
export const FULL_SLIP_RADIANS = 0.32

/** Lateral sleeve travel in face-scaled pixels, shared by both sleeves. */
export const ARM_SWING_TRAVEL = 6

/**
 * Extra travel for whichever sleeve the torso is carrying forward: it is the
 * nearer one, sweeps the wider arc, and so falls further behind. Kept below
 * the shared travel so the two sleeves never move against each other.
 */
export const ARM_SWING_SPREAD = 2.2

/**
 * A pendulum rises by L(1 - cos t) as it deflects — second order in the
 * deflection. The lift is derived from the swing rather than authored beside
 * it, so it can never disagree with it.
 */
export const ARM_SWING_LIFT = 0.09

export interface ArmFollowState {
  /** Normalized lateral lag, signed against the torso's turn. */
  swing: number
  /** Contribution to `armY`, always a lift. */
  lift: number
}

/**
 * One passive follower for both sleeves. The output is the torso rotation the
 * arm has *not* caught up with, which is what a trailing limb shows.
 */
export class ArmFollowController {
  private initialized = false
  private position = 0
  private velocity = 0
  private readonly state: ArmFollowState = { swing: 0, lift: 0 }

  /**
   * `torsoYaw` is the torso's actual rotation, not the pose it is heading to:
   * a passive follower driven by a goal the body has not reached yet would
   * swing before the body did. `limit` is the asset's rigid-arm allowance, so
   * a portrait with no sleeve layer resolves to no swing at all.
   */
  step(
    torsoYaw: number,
    elapsedSeconds: number,
    limit: number,
  ): Readonly<ArmFollowState> {
    const yaw = Number.isFinite(torsoYaw) ? torsoYaw : 0
    const dt = Number.isFinite(elapsedSeconds) ? Math.max(0, elapsedSeconds) : 0
    if (!this.initialized) {
      this.initialized = true
      this.position = yaw
      this.velocity = 0
    }
    // Exact solution of x'' = w²(yaw - x) - 2wx' for this step: two coincident
    // real poles, so the sleeve trails without ever overshooting, and the same
    // elapsed time gives the same lag however the frame rate split it.
    const omega = 2 * Math.PI * ARM_FOLLOW_HZ
    const offset = this.position - yaw
    const c1 = this.velocity + omega * offset
    const polynomial = offset + c1 * dt
    const decay = Math.exp(-omega * dt)
    this.position = yaw + polynomial * decay
    this.velocity = (c1 - omega * polynomial) * decay
    const bound = Math.max(0, Number.isFinite(limit) ? limit : 0)
    // The sleeve sits where the torso *was*, so its offset from where the
    // torso is now runs against the turn.
    const lag = clamp((this.position - yaw) / FULL_SLIP_RADIANS, -1, 1)
    this.state.swing = clamp(lag, -bound, bound)
    this.state.lift = this.state.swing * this.state.swing * ARM_SWING_LIFT
    return this.state
  }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value))
}
