import { MAX_RIGID_ARM_ROTATION_DEGREES } from '../rig/contract'

/** `armY` = 1 opens both arms this far away from the body. */
export const ARM_OPEN_RADIANS = (14 * Math.PI) / 180

/** `armPos` = 1 swings both arms this far toward image right. */
export const ARM_SWAY_RADIANS = (14 * Math.PI) / 180

/** The asset contract's promise; the swing saturates softly into it. */
export const ARM_MAX_RADIANS = (MAX_RIGID_ARM_ROTATION_DEGREES * Math.PI) / 180

/** How much of a body roll the hanging arms give back to gravity. */
export const ARM_HANG = 0.5

export const ARM_PENDULUM_HZ = 1.6
export const ARM_PENDULUM_DAMPING = 0.4

/**
 * A torso twist drawn in 2D moves the shoulder far less than the turning body
 * moves the arm in depth; the drawn shoulder's acceleration under-reads it.
 */
export const ARM_INERTIA_GAIN = 4.5

/** Shoulder acceleration above this is a discontinuity, not motion. */
const MAX_SUPPORT_ACCELERATION = 20_000

/** The two arms are not one rigid bar; a small detune keeps them out of lockstep. */
const SIDE_DETUNE = 0.04

export interface ArmPendulumInput {
  /** `armY` after composition and the motion envelope. */
  open: number
  /** `armPos` after composition and the motion envelope. */
  sway: number
  /** The shader-owned body rotation, in radians. */
  bodyRoll: number
  /** False freezes dynamics: arms sit exactly on their intent. */
  dynamic: boolean
}

export interface ArmSupport {
  /** World position of the shoulder joint after all primary motion. */
  x: number
  y: number
  reach: number
}

/** One hanging arm per side. `outward` is the rotation sign away from the body. */
export class ArmPendulum {
  angle = 0
  private state = 0
  private velocity = 0
  private initialized = false
  private lastX = 0
  private lastY = 0
  private supportVelocityX = 0
  private supportVelocityY = 0
  private hasSupportVelocity = false

  constructor(private readonly outward: 1 | -1) {}

  /** Image-plane rotation of the arm about its shoulder, in radians. */
  step(input: Readonly<ArmPendulumInput>, support: Readonly<ArmSupport> | null, dt: number): number {
    const open = finite(input.open)
    const sway = finite(input.sway)
    const roll = finite(input.bodyRoll)
    // Positive rotation moves a hanging arm's hand toward image left.
    const target = this.outward * open * ARM_OPEN_RADIANS - sway * ARM_SWAY_RADIANS - ARM_HANG * roll
    const step = Number.isFinite(dt) ? Math.max(0, dt) : 0
    if (!this.initialized || !input.dynamic) {
      this.initialized = true
      this.velocity = 0
      this.state = target
      this.forgetSupport(support)
      return this.output()
    }
    let accelerationX = 0
    let accelerationY = 0
    if (support && step > 0 && Number.isFinite(support.x) && Number.isFinite(support.y)) {
      const vx = (support.x - this.lastX) / step
      const vy = (support.y - this.lastY) / step
      if (this.hasSupportVelocity) {
        accelerationX = clamp((vx - this.supportVelocityX) / step, -MAX_SUPPORT_ACCELERATION, MAX_SUPPORT_ACCELERATION)
        accelerationY = clamp((vy - this.supportVelocityY) / step, -MAX_SUPPORT_ACCELERATION, MAX_SUPPORT_ACCELERATION)
      }
      this.lastX = support.x
      this.lastY = support.y
      this.supportVelocityX = vx
      this.supportVelocityY = vy
      this.hasSupportVelocity = true
    } else {
      this.forgetSupport(support)
    }
    const omega = 2 * Math.PI * ARM_PENDULUM_HZ * (1 + this.outward * SIDE_DETUNE)
    const reach = support && support.reach > 0 ? support.reach : Infinity
    const world = this.state + roll
    // A pendulum on a moving support: the hand lags the shoulder's acceleration.
    const inertia =
      (ARM_INERTIA_GAIN * (accelerationX * Math.cos(world) + accelerationY * Math.sin(world))) / reach
    const acceleration =
      -omega * omega * (this.state - target) - 2 * ARM_PENDULUM_DAMPING * omega * this.velocity + inertia
    this.velocity += acceleration * step
    this.state += this.velocity * step
    if (!Number.isFinite(this.state) || !Number.isFinite(this.velocity)) {
      this.state = target
      this.velocity = 0
    }
    return this.output()
  }

  private output(): number {
    this.angle = ARM_MAX_RADIANS * Math.tanh(this.state / ARM_MAX_RADIANS)
    return this.angle
  }

  private forgetSupport(support: Readonly<ArmSupport> | null): void {
    this.hasSupportVelocity = false
    this.supportVelocityX = 0
    this.supportVelocityY = 0
    if (support && Number.isFinite(support.x) && Number.isFinite(support.y)) {
      this.lastX = support.x
      this.lastY = support.y
    }
  }
}

export const DRAPE_HZ = 0.85
export const DRAPE_DAMPING = 0.35

/** Gravity holds a hanging drape this much nearer vertical than its arm. */
export const DRAPE_SAG = 0.3

/** Cloth hung from an arm: softer than the arm, it trails the swing and sags. */
export class ArmDrape {
  angle = 0
  private state = 0
  private velocity = 0
  private initialized = false

  /** `armAngle` and the result are rotations about the same shoulder. */
  step(armAngle: number, bodyRoll: number, dynamic: boolean, dt: number): number {
    const arm = finite(armAngle)
    // In the body's frame, world vertical is the body roll undone.
    const target = arm * (1 - DRAPE_SAG) - finite(bodyRoll) * DRAPE_SAG
    const step = Number.isFinite(dt) ? Math.max(0, dt) : 0
    if (!this.initialized || !dynamic) {
      this.initialized = true
      this.state = target
      this.velocity = 0
    } else {
      const omega = 2 * Math.PI * DRAPE_HZ
      this.velocity += (-omega * omega * (this.state - target) - 2 * DRAPE_DAMPING * omega * this.velocity) * step
      this.state += this.velocity * step
      if (!Number.isFinite(this.state) || !Number.isFinite(this.velocity)) {
        this.state = target
        this.velocity = 0
      }
    }
    this.angle = ARM_MAX_RADIANS * Math.tanh(this.state / ARM_MAX_RADIANS)
    return this.angle
  }
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value))
}
