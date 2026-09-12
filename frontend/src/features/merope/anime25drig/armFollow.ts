export const ARM_FOLLOW_HZ = 1.1

export const FULL_SLIP_RADIANS = 0.32

export const ARM_SWING_TRAVEL = 6

/** Kept below the shared travel so the two sleeves never move against each other. */
export const ARM_SWING_SPREAD = 2.2

export const ARM_SWING_LIFT = 0.09

export interface ArmFollowState {
  swing: number
  lift: number
}

export class ArmFollowController {
  private initialized = false
  private position = 0
  private velocity = 0
  private readonly state: ArmFollowState = { swing: 0, lift: 0 }

  /** `torsoYaw` is the torso's actual rotation, not the pose it is heading to */
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
    const omega = 2 * Math.PI * ARM_FOLLOW_HZ
    const offset = this.position - yaw
    const c1 = this.velocity + omega * offset
    const polynomial = offset + c1 * dt
    const decay = Math.exp(-omega * dt)
    this.position = yaw + polynomial * decay
    this.velocity = (c1 - omega * polynomial) * decay
    const bound = Math.max(0, Number.isFinite(limit) ? limit : 0)
    const lag = clamp((this.position - yaw) / FULL_SLIP_RADIANS, -1, 1)
    this.state.swing = clamp(lag, -bound, bound)
    this.state.lift = this.state.swing * this.state.swing * ARM_SWING_LIFT
    return this.state
  }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value))
}
