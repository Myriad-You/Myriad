/**
 * A lock of hair as a chain of short links hanging from its root. When the
 * head moves, the first link is pulled along and each link below follows
 * the one above it a moment later, so the motion travels down the lock and
 * the tip whips through an S rather than the whole lock leaning as one
 * piece. Links keep their length: a swinging tip rises on its arc instead of
 * stretching, as Live2D's physics chains do.
 *
 * Each link is sprung toward the direction it hangs in at rest, partly
 * carried round by the link above it (a lock holds its own curve), and
 * damped against the link above and, as air does, against the root's
 * motion, so carrying the whole lock costs nothing and only its swing is
 * slowed.
 */

export interface HairChainTuning {
  /** Angular frequency of a link's spring, rad/s. */
  omega: number
  /** Damping ratio of each link against the one above it. */
  damping: number
  /** How much of the link above's bend a link keeps: 0 hangs straight, 1 holds the curve. */
  carry: number
  /** Toward the tip the links soften to this share of the root's stiffness. */
  tipStiffness: number
  /** Air drag on the lock's swing, per second, against the root's motion. */
  drag: number
}

export interface HairChain {
  readonly links: number
  readonly linkLength: number
  /** Unit direction the lock hangs in at rest, root toward tip. */
  readonly restX: number
  readonly restY: number
  readonly x: Float64Array
  readonly y: Float64Array
  readonly vx: Float64Array
  readonly vy: Float64Array
  /** Each particle's offset from where the rigidly carried lock would put it. */
  readonly offsetX: Float32Array
  readonly offsetY: Float32Array
  settled: boolean
}

const MAX_STEP_SECONDS = 1 / 240
const MAX_ELAPSED_SECONDS = 0.05
/** A root that jumps this share of the lock's length in one frame was moved, not swung. */
const TELEPORT_SHARE = 0.5

export function createHairChain(
  rootX: number,
  rootY: number,
  tipX: number,
  tipY: number,
  links: number,
): HairChain {
  const count = Math.max(1, Math.round(links))
  const length = Math.hypot(tipX - rootX, tipY - rootY)
  const safeLength = length > 1e-6 ? length : 1
  const chain: HairChain = {
    links: count,
    linkLength: safeLength / count,
    restX: length > 1e-6 ? (tipX - rootX) / length : 0,
    restY: length > 1e-6 ? (tipY - rootY) / length : 1,
    x: new Float64Array(count + 1),
    y: new Float64Array(count + 1),
    vx: new Float64Array(count + 1),
    vy: new Float64Array(count + 1),
    offsetX: new Float32Array(count + 1),
    offsetY: new Float32Array(count + 1),
    settled: false,
  }
  return chain
}

/** Hangs the lock at rest from `rootX/Y`, at rest and still. */
export function resetHairChain(chain: HairChain, rootX: number, rootY: number): void {
  for (let index = 0; index <= chain.links; index++) {
    chain.x[index] = rootX + chain.restX * chain.linkLength * index
    chain.y[index] = rootY + chain.restY * chain.linkLength * index
    chain.vx[index] = 0
    chain.vy[index] = 0
  }
  chain.offsetX.fill(0)
  chain.offsetY.fill(0)
  chain.settled = true
}

/**
 * Moves the root to `rootX/Y` over `elapsed` seconds and lets the lock
 * follow. `windX` is a sideways push on the lock, in px/s², growing toward
 * the tip.
 */
export function stepHairChain(
  chain: HairChain,
  rootX: number,
  rootY: number,
  tuning: Readonly<HairChainTuning>,
  windX: number,
  elapsed: number,
): void {
  if (!Number.isFinite(rootX) || !Number.isFinite(rootY)) return
  const total = chain.linkLength * chain.links
  if (!chain.settled || Math.hypot(rootX - chain.x[0], rootY - chain.y[0]) > total * TELEPORT_SHARE) {
    resetHairChain(chain, rootX, rootY)
    return
  }
  if (!Number.isFinite(elapsed) || elapsed <= 0) return
  const bounded = Math.min(elapsed, MAX_ELAPSED_SECONDS)
  const steps = Math.max(1, Math.ceil(bounded / MAX_STEP_SECONDS))
  const h = bounded / steps
  const { x, y, vx, vy, links, linkLength, restX, restY } = chain
  const startX = x[0]
  const startY = y[0]
  const omega = Math.max(0, tuning.omega)
  const carry = Math.max(0, Math.min(1, tuning.carry))
  for (let step = 1; step <= steps; step++) {
    // The root is carried; it is not simulated.
    const t = step / steps
    const nextRootX = startX + (rootX - startX) * t
    const nextRootY = startY + (rootY - startY) * t
    vx[0] = (nextRootX - x[0]) / h
    vy[0] = (nextRootY - y[0]) / h
    x[0] = nextRootX
    y[0] = nextRootY
    let parentX = restX
    let parentY = restY
    for (let index = 1; index <= links; index++) {
      const share = links > 1 ? (index - 1) / (links - 1) : 0
      const stiffness = omega * omega * (1 + (tuning.tipStiffness - 1) * share)
      const damping = 2 * tuning.damping * Math.sqrt(Math.max(0, stiffness))
      // The direction this link wants: its rest direction, turned by part of the bend above it.
      const bend = Math.atan2(parentY, parentX) - Math.atan2(restY, restX)
      const turn = wrapAngle(bend) * carry
      const cosine = Math.cos(turn)
      const sine = Math.sin(turn)
      const wantX = restX * cosine - restY * sine
      const wantY = restX * sine + restY * cosine
      const targetX = x[index - 1] + wantX * linkLength
      const targetY = y[index - 1] + wantY * linkLength
      const ax = stiffness * (targetX - x[index]) - damping * (vx[index] - vx[index - 1]) -
        tuning.drag * (vx[index] - vx[0]) + windX * (index / links)
      const ay = stiffness * (targetY - y[index]) - damping * (vy[index] - vy[index - 1]) -
        tuning.drag * (vy[index] - vy[0])
      const beforeX = x[index]
      const beforeY = y[index]
      vx[index] += ax * h
      vy[index] += ay * h
      let nextX = beforeX + vx[index] * h
      let nextY = beforeY + vy[index] * h
      // The link keeps its length.
      const dx = nextX - x[index - 1]
      const dy = nextY - y[index - 1]
      const distance = Math.hypot(dx, dy)
      if (distance > 1e-9) {
        nextX = x[index - 1] + (dx / distance) * linkLength
        nextY = y[index - 1] + (dy / distance) * linkLength
      } else {
        nextX = x[index - 1] + wantX * linkLength
        nextY = y[index - 1] + wantY * linkLength
      }
      vx[index] = (nextX - beforeX) / h
      vy[index] = (nextY - beforeY) / h
      x[index] = nextX
      y[index] = nextY
      parentX = (nextX - x[index - 1]) / linkLength
      parentY = (nextY - y[index - 1]) / linkLength
    }
  }
  let finite = true
  for (let index = 0; index <= links; index++) {
    const offsetX = x[index] - (x[0] + restX * linkLength * index)
    const offsetY = y[index] - (y[0] + restY * linkLength * index)
    if (!Number.isFinite(offsetX) || !Number.isFinite(offsetY)) finite = false
    chain.offsetX[index] = offsetX
    chain.offsetY[index] = offsetY
  }
  if (!finite) resetHairChain(chain, rootX, rootY)
}

/** The lock's offset from its rigidly carried rest, at `along` (0 root … 1 tip). */
export function hairChainOffset(
  chain: Readonly<HairChain>,
  along: number,
  out: { x: number; y: number },
): { x: number; y: number } {
  const position = Math.max(0, Math.min(1, along)) * chain.links
  const index = Math.min(chain.links - 1, Math.floor(position))
  const t = position - index
  out.x = chain.offsetX[index] + (chain.offsetX[index + 1] - chain.offsetX[index]) * t
  out.y = chain.offsetY[index] + (chain.offsetY[index + 1] - chain.offsetY[index]) * t
  return out
}

/**
 * How far the lock has turned from hanging as drawn at `along`, radians.
 * Taken at the middle of each link and blended between them, so a lock's
 * width turns with it smoothly, as a ribbon bends.
 */
export function hairChainTurn(chain: Readonly<HairChain>, along: number): number {
  const position = Math.max(0, Math.min(1, along)) * chain.links - 0.5
  const first = Math.max(0, Math.min(chain.links - 1, Math.floor(position)))
  const second = Math.min(chain.links - 1, first + 1)
  const t = Math.max(0, Math.min(1, position - first))
  const turn = linkTurn(chain, first)
  return turn + wrapAngle(linkTurn(chain, second) - turn) * t
}

function linkTurn(chain: Readonly<HairChain>, link: number): number {
  const dx = chain.x[link + 1] - chain.x[link]
  const dy = chain.y[link + 1] - chain.y[link]
  if (dx === 0 && dy === 0) return 0
  return wrapAngle(Math.atan2(dy, dx) - Math.atan2(chain.restY, chain.restX))
}

function wrapAngle(angle: number): number {
  if (angle > Math.PI) return angle - 2 * Math.PI
  if (angle < -Math.PI) return angle + 2 * Math.PI
  return angle
}
