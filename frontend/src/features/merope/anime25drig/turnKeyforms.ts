import type { HeadTurn } from './headTurn'
import type { Anime25DPlaybackLayer } from './types'

/**
 * A head turn drawn, not computed: as a Live2D rigger keys a warp deformer at
 * AngleX ±30, each part of the head has the shape it takes at the full turn
 * either way, as a lattice over its bounds. A turn in between is the rest
 * shape and the key mixed linearly by the angle.
 */
export interface Anime25DTurnLattice {
  /** Canvas bounds the lattice spans in the turned drawing: x0, y0, x1, y1. */
  box: [number, number, number, number]
  /** Points per side. */
  grid: number
  /**
   * For each point of the turned drawing, how far back to where it was at
   * rest (x, y per point, row by row). Keyed this way round, a lattice point
   * stands for one place in the turned picture; each rest vertex finds its
   * place by inverting it.
   */
  back: number[]
  /**
   * The part's colour at the full turn, as a multiple of its colour at rest
   * (r, g, b): the light on it changes as it turns (a neck the raised chin
   * no longer shades). Cubism keys a multiply colour the same way.
   */
  multiply?: [number, number, number]
}

/** A part's key at the full turn toward +x and toward −x. */
export interface Anime25DTurnKey {
  plus: Anime25DTurnLattice
  minus: Anime25DTurnLattice
  /** At the full nod raising the face, and lowering it; without them the nod is computed. */
  up?: Anime25DTurnLattice
  down?: Anime25DTurnLattice
}

/** Keyed by part family (turnKeyformFamily). */
export type Anime25DTurnKeyforms = Record<string, Anime25DTurnKey>

type KeyedLayer = Pick<Anime25DPlaybackLayer, 'group' | 'role' | 'side'> & { name?: string }

/** Which keyed part a layer turns with: every drawing of one eye with that eye's key. */
export function turnKeyformFamily(layer: KeyedLayer): string | null {
  const { role, side } = layer
  if (role === 'neck' || role === 'neckwear') return role
  if (layer.group !== 'head') return null
  if (role === 'back-hair' || role === 'front-hair' || role === 'headwear' || role === 'ears' || role === 'earwear') return role
  if (role === 'nose') return 'nose'
  if (role === 'eyebrow') return side ? `brow:${side}` : 'face'
  if (side && /eye|iris|irides|lovestruck-heart/.test(role)) return `eye:${side}`
  if (/mouth|drool/.test(role)) return 'mouth'
  return 'face'
}

/**
 * A lock of front hair cut into its own layer ('front-hair-N') turns by its
 * own key ('front-hair:N') where one was measured, else with the whole front
 * hair. An iris (and what is drawn in its place) turns by its own key
 * ('iris:L'/'iris:R') where one was measured, moving inside the white, else
 * with the whole eye. A mouth, nose or brow without a key turns with the face.
 */
function turnKeyFor(keyforms: Readonly<Anime25DTurnKeyforms> | undefined, layer: KeyedLayer): Anime25DTurnKey | undefined {
  const family = turnKeyformFamily(layer)
  if (!family || !keyforms) return undefined
  const lock = family === 'front-hair' ? /^front-hair-(\d+)$/.exec(layer.name ?? '') : null
  const iris = family.startsWith('eye:') && /iris|irides|lovestruck-heart/.test(layer.role) ? `iris:${layer.side}` : null
  const own = lock ? `front-hair:${lock[1]}` : iris
  const key = (own ? keyforms[own] : undefined) ?? keyforms[family]
  if (!ON_THE_FACE.test(family)) return key
  if (!key) return keyforms.face
  const face = keyforms.face
  if ((key.up && key.down) || !face?.up || !face.down) return key
  return { ...key, up: { ...face.up, multiply: undefined }, down: { ...face.down, multiply: undefined } }
}

/**
 * Features drawn on the face turn with it when they have no key of their
 * own: a mouth too small to be found in every turned decomposition is not
 * fitted at all, and turned by the computed head it slides off a keyed face.
 * One keyed across but not nodding nods with the skin under it, else the
 * computed nod leaves it behind while the eyes and the face go down, and the
 * features crowd together (or spread apart, the face raised).
 */
const ON_THE_FACE = /^(mouth|nose|brow:[LR])$/

export interface BoundTurnKeyform {
  plus: Float32Array
  minus: Float32Array
  up: Float32Array | null
  down: Float32Array | null
  /**
   * At the full turn and the full nod together, per turn side and nod side
   * (turnedNod). Null without a nod key, or where a caller built the turn by
   * hand.
   */
  corners?: { plusUp: Float32Array; plusDown: Float32Array; minusUp: Float32Array; minusDown: Float32Array } | null
  /** The multiply colour at each full key (r, g, b), when the keys hold one. */
  multiply: { plus: Float32Array; minus: Float32Array; up: Float32Array | null; down: Float32Array | null } | null
  /** A body key's share per vertex of the torso's cylinder in place of the key (legHipWeights). */
  cylinder?: Float32Array | null
}

/** Each vertex's offset at the full turn either way: where in the turned drawing its rest point went. */
export function bindTurnKeyform(
  keyforms: Readonly<Anime25DTurnKeyforms> | undefined,
  layer: KeyedLayer,
  rest: Float32Array,
): BoundTurnKeyform | null {
  const key = turnKeyFor(keyforms, layer)
  if (!key) return null
  const bound = bindTurnKey(key, rest)
  return turnKeyformFamily(layer) === 'mouth' ? unsheared(bound, rest) : bound
}

/** A mouth's key scales it by at most this much either way: turned 30°, it narrows by cos 30°. */
const MOUTH_SCALE = 0.25

/**
 * A mouth's key, as a move and a stretch along each axis only. It is fitted
 * on the mouth the turned drawings drew, often another shape than the one
 * shown: an open mouth over the closed one's fit reaches lattice cells only
 * the fit's smoothing set, and came out sheared into a hard parallelogram.
 * Per key, the offsets are fitted over the layer's own points as
 * t + s·(p − centre) per axis, which keeps the mouth's place and its
 * narrowing and drops the shear.
 */
function unsheared(bound: BoundTurnKeyform, rest: Float32Array): BoundTurnKeyform {
  const n = rest.length / 2
  let cx = 0
  let cy = 0
  for (let v = 0; v < n; v++) {
    cx += rest[v * 2] / n
    cy += rest[v * 2 + 1] / n
  }
  const fit = (offsets: Float32Array) => {
    const out = new Float32Array(offsets.length)
    for (const axis of [0, 1]) {
      const centre = axis === 0 ? cx : cy
      let mean = 0
      for (let v = 0; v < n; v++) mean += offsets[v * 2 + axis] / n
      let covariance = 0
      let variance = 0
      for (let v = 0; v < n; v++) {
        const d = rest[v * 2 + axis] - centre
        covariance += d * (offsets[v * 2 + axis] - mean)
        variance += d * d
      }
      // A mouth drawn as a line has no height to stretch.
      const scale = variance / n < 1 ? 0 : Math.max(-MOUTH_SCALE, Math.min(MOUTH_SCALE, covariance / variance))
      for (let v = 0; v < n; v++) out[v * 2 + axis] = mean + scale * (rest[v * 2 + axis] - centre)
    }
    return out
  }
  const corners = bound.corners
  return {
    ...bound,
    plus: fit(bound.plus),
    minus: fit(bound.minus),
    up: bound.up && fit(bound.up),
    down: bound.down && fit(bound.down),
    corners: corners && { plusUp: fit(corners.plusUp), plusDown: fit(corners.plusDown), minusUp: fit(corners.minusUp), minusDown: fit(corners.minusDown) },
  }
}

/** A key's offsets at each rest point (x, y per point). */
export function bindTurnKey(key: Readonly<Anime25DTurnKey>, rest: Float32Array): BoundTurnKeyform {
  const nod = Boolean(key.up && key.down)
  const plus = invertLattice(key.plus, rest)
  const minus = invertLattice(key.minus, rest)
  return {
    plus,
    minus,
    up: nod ? invertLattice(key.up!, rest) : null,
    down: nod ? invertLattice(key.down!, rest) : null,
    corners: nod ? turnedNod(key, rest, plus, minus) : null,
    multiply: key.plus.multiply || key.minus.multiply || (nod && (key.up!.multiply || key.down!.multiply))
      ? {
          plus: multiplyOf(key.plus),
          minus: multiplyOf(key.minus),
          up: nod ? multiplyOf(key.up!) : null,
          down: nod ? multiplyOf(key.down!) : null,
        }
      : null,
  }
}

/**
 * The corner keys, composed rather than summed: each rest point turned, then
 * nodded as the nod key moves the place it was turned to. Summed, a turn and
 * a nod that each narrow a part along the same line (the far eye by the turn
 * and its lid by the nod) take its width away twice over, and it folds;
 * composed, the nod narrows what the turn left. The place is taken less the
 * part's whole move in the turn, so a small part's nod key is read on the
 * part, not across the face where it was never fitted.
 */
function turnedNod(key: Readonly<Anime25DTurnKey>, rest: Float32Array, plus: Float32Array, minus: Float32Array) {
  const corner = (turn: Float32Array, lattice: Readonly<Anime25DTurnLattice>, nod: Readonly<Anime25DTurnLattice>) => {
    const [mx, my] = latticeMove(lattice)
    const turned = new Float32Array(rest.length)
    for (let i = 0; i < rest.length; i += 2) {
      turned[i] = rest[i] + turn[i] - mx
      turned[i + 1] = rest[i + 1] + turn[i + 1] - my
    }
    const nodded = invertLattice(nod, turned)
    for (let i = 0; i < rest.length; i++) nodded[i] += turn[i]
    return nodded
  }
  return {
    plusUp: corner(plus, key.plus, key.up!),
    plusDown: corner(plus, key.plus, key.down!),
    minusUp: corner(minus, key.minus, key.up!),
    minusDown: corner(minus, key.minus, key.down!),
  }
}

/** A key's whole move: its lattice points, turned, less where they were at rest, on average. */
function latticeMove(lattice: Readonly<Anime25DTurnLattice>): [number, number] {
  let x = 0
  let y = 0
  for (let n = 0; n < lattice.back.length; n += 2) {
    x -= lattice.back[n]
    y -= lattice.back[n + 1]
  }
  const count = lattice.back.length / 2
  return [x / count, y / count]
}

/** A key's multiply colour, white without one; held to a sane range. */
function multiplyOf(lattice: Readonly<Anime25DTurnLattice>): Float32Array {
  const m = lattice.multiply
  return Float32Array.from([0, 1, 2], (i) =>
    m && Number.isFinite(m[i]) ? Math.max(MULTIPLY_MIN, Math.min(MULTIPLY_MAX, m[i])) : 1)
}

const MULTIPLY_MIN = 0.5
const MULTIPLY_MAX = 1.5

/** The multiply colour at a turn and a nod: rest's white, mixed toward each key by its share. */
export function turnKeyformMultiply(
  bound: Readonly<BoundTurnKeyform>,
  amount: number,
  nod: number,
  out: Float32Array,
): void {
  out[0] = out[1] = out[2] = 1
  const m = bound.multiply
  if (!m) return
  const key = amount >= 0 ? m.plus : m.minus
  const share = Math.min(1, Math.abs(amount))
  const nodKey = nod >= 0 ? m.up : m.down
  const nodShare = nodKey ? Math.min(1, Math.abs(nod)) : 0
  for (let i = 0; i < 3; i++) {
    out[i] = 1 + (key[i] - 1) * share + (nodKey ? (nodKey[i] - 1) * nodShare : 0)
  }
}

/**
 * For each rest point q, the turned point t with t + back(t) = q, as t − q.
 * Stepping t ← q − back(t) finds it wherever the key moves its neighbourhood
 * less than it moves itself; where a part is squeezed hard (the far eye at
 * the full turn) the steps overshoot and swing back and forth for ever, so
 * there t is solved on the lattice itself (latticeRoot).
 */
function invertLattice(lattice: Readonly<Anime25DTurnLattice>, rest: Float32Array): Float32Array {
  const out = new Float32Array(rest.length)
  const at = new Float32Array(2)
  let cells: LatticeCells | null = null
  for (let i = 0; i < rest.length; i += 2) {
    const qx = rest[i]
    const qy = rest[i + 1]
    let tx = qx
    let ty = qy
    let settled = false
    for (let step = 0; step < 30; step++) {
      sampleLattice(lattice, tx, ty, at)
      const nx = qx - at[0]
      const ny = qy - at[1]
      const moved = Math.abs(nx - tx) + Math.abs(ny - ty)
      tx = nx
      ty = ny
      if (moved < 0.01) {
        settled = true
        break
      }
    }
    if (!settled) {
      cells ??= latticeCells(lattice)
      if (latticeRoot(cells, qx, qy, at)) {
        tx = at[0]
        ty = at[1]
      }
    }
    out[i] = tx - qx
    out[i + 1] = ty - qy
  }
  return out
}

/**
 * The lattice as cells, each with where its corners land at rest (t + back(t)),
 * ringed by cells reaching FAR beyond the box that carry the edge's offsets
 * outward, as sampleLattice holds them past the box. Within a cell t + back(t)
 * is bilinear, so a rest point's turned point solves a quadratic there.
 */
interface LatticeCells {
  xs: Float64Array
  ys: Float64Array
  /** Landing point per node of the ringed lattice (x, y per node, row by row). */
  landed: Float64Array
  /** Bounds of each cell's landed corners: x0, y0, x1, y1 per cell. */
  bounds: Float64Array
}

const FAR = 1e5

function latticeCells(lattice: Readonly<Anime25DTurnLattice>): LatticeCells {
  const [x0, y0, x1, y1] = lattice.box
  const grid = lattice.grid
  const side = grid + 2
  const xs = new Float64Array(side)
  const ys = new Float64Array(side)
  for (let k = 0; k < grid; k++) {
    xs[k + 1] = x0 + ((x1 - x0) * k) / (grid - 1)
    ys[k + 1] = y0 + ((y1 - y0) * k) / (grid - 1)
  }
  xs[0] = x0 - FAR
  ys[0] = y0 - FAR
  xs[side - 1] = x1 + FAR
  ys[side - 1] = y1 + FAR
  const landed = new Float64Array(side * side * 2)
  for (let j = 0; j < side; j++) {
    const lj = Math.max(0, Math.min(grid - 1, j - 1))
    for (let i = 0; i < side; i++) {
      const li = Math.max(0, Math.min(grid - 1, i - 1))
      const n = (j * side + i) * 2
      landed[n] = xs[i] + lattice.back[(lj * grid + li) * 2]
      landed[n + 1] = ys[j] + lattice.back[(lj * grid + li) * 2 + 1]
    }
  }
  const bounds = new Float64Array((side - 1) * (side - 1) * 4)
  for (let j = 0; j < side - 1; j++) {
    for (let i = 0; i < side - 1; i++) {
      const b = (j * (side - 1) + i) * 4
      bounds[b] = bounds[b + 1] = Infinity
      bounds[b + 2] = bounds[b + 3] = -Infinity
      for (const n of [j * side + i, j * side + i + 1, (j + 1) * side + i, (j + 1) * side + i + 1]) {
        bounds[b] = Math.min(bounds[b], landed[n * 2])
        bounds[b + 1] = Math.min(bounds[b + 1], landed[n * 2 + 1])
        bounds[b + 2] = Math.max(bounds[b + 2], landed[n * 2])
        bounds[b + 3] = Math.max(bounds[b + 3], landed[n * 2 + 1])
      }
    }
  }
  return { xs, ys, landed, bounds }
}

/**
 * The turned point landing on (qx, qy), solved cell by cell; where a key
 * folds and several do, the one nearest the rest point. False if none does.
 */
function latticeRoot(cells: LatticeCells, qx: number, qy: number, out: Float32Array): boolean {
  const { xs, ys, landed, bounds } = cells
  const side = xs.length
  let best = Infinity
  for (let j = 0; j < side - 1; j++) {
    for (let i = 0; i < side - 1; i++) {
      const b = (j * (side - 1) + i) * 4
      const slack = 1e-6 * (1 + bounds[b + 2] - bounds[b] + bounds[b + 3] - bounds[b + 1])
      if (qx < bounds[b] - slack || qy < bounds[b + 1] - slack || qx > bounds[b + 2] + slack || qy > bounds[b + 3] + slack) continue
      const a = (j * side + i) * 2
      const r = a + 2
      const d = a + side * 2
      const c = d + 2
      const ex = landed[r] - landed[a]
      const ey = landed[r + 1] - landed[a + 1]
      const fx = landed[d] - landed[a]
      const fy = landed[d + 1] - landed[a + 1]
      const gx = landed[a] - landed[r] + landed[c] - landed[d]
      const gy = landed[a + 1] - landed[r + 1] + landed[c + 1] - landed[d + 1]
      const hx = qx - landed[a]
      const hy = qy - landed[a + 1]
      // h = u·e + v·f + u·v·g, crossed with the cell's sides: k2·v² + k1·v + k0 = 0.
      const k2 = gx * fy - gy * fx
      const k1 = ex * fy - ey * fx + hx * gy - hy * gx
      const k0 = hx * ey - hy * ex
      const scale = Math.abs(k1) + Math.abs(k2) + Math.abs(k0)
      if (scale === 0) continue
      let roots: number[]
      if (Math.abs(k2) <= 1e-12 * scale) {
        if (Math.abs(k1) <= 1e-12 * scale) continue
        roots = [-k0 / k1]
      } else {
        const disc = k1 * k1 - 4 * k0 * k2
        if (disc < 0) continue
        const w = Math.sqrt(disc)
        roots = [(-k1 - w) / (2 * k2), (-k1 + w) / (2 * k2)]
      }
      for (const v of roots) {
        if (!(v >= -1e-9 && v <= 1 + 1e-9)) continue
        const sx = ex + gx * v
        const sy = ey + gy * v
        const along = sx * sx + sy * sy
        if (along === 0) continue
        const u = ((hx - fx * v) * sx + (hy - fy * v) * sy) / along
        if (!(u >= -1e-9 && u <= 1 + 1e-9)) continue
        const tx = xs[i] + (xs[i + 1] - xs[i]) * u
        const ty = ys[j] + (ys[j + 1] - ys[j]) * v
        const away = (tx - qx) ** 2 + (ty - qy) ** 2
        if (away < best) {
          best = away
          out[0] = tx
          out[1] = ty
        }
      }
    }
  }
  return best < Infinity
}

/**
 * The offset at `amount` of the full turn (−1…1) and `nod` of the full nod,
 * for vertex `vertex`. Along either axis it is that key mixed by its share;
 * in between, the four keys around the pose mix bilinearly, as a Live2D
 * deformer mixes its keyforms, the corner composed (turnedNod).
 */
export function turnKeyformOffset(
  bound: Readonly<BoundTurnKeyform>,
  vertex: number,
  amount: number,
  out: { x: number; y: number },
  nod = 0,
): void {
  const key = amount >= 0 ? bound.plus : bound.minus
  const share = Math.min(1, Math.abs(amount))
  out.x = key[vertex * 2] * share
  out.y = key[vertex * 2 + 1] * share
  const nodKey = nod >= 0 ? bound.up : bound.down
  if (!nodKey || nod === 0) return
  const nodShare = Math.min(1, Math.abs(nod))
  const nx = nodKey[vertex * 2]
  const ny = nodKey[vertex * 2 + 1]
  out.x += nx * nodShare
  out.y += ny * nodShare
  const corners = bound.corners
  if (!corners || share === 0) return
  const corner = amount >= 0 ? (nod >= 0 ? corners.plusUp : corners.plusDown) : nod >= 0 ? corners.minusUp : corners.minusDown
  const both = share * nodShare
  out.x += (corner[vertex * 2] - key[vertex * 2] - nx) * both
  out.y += (corner[vertex * 2 + 1] - key[vertex * 2 + 1] - ny) * both
}

/** The computed head a keyed part still takes: nodding only, or held still when the nod is keyed too. */
export function unkeyedTurn(turn: Readonly<HeadTurn> | undefined, bound: Readonly<BoundTurnKeyform>): Readonly<HeadTurn> | undefined {
  if (!turn) return undefined
  return (bound.up ? turn.still : turn.nodOnly) ?? undefined
}

export interface AttachmentTurn {
  own: BoundTurnKeyform
  /** The host's key at the anchor and one pixel across from it. */
  host: BoundTurnKeyform | null
  anchor: { x: number; y: number }
  amount: number
  nod: number
}

/** A keyed accessory's turn: its own key per vertex, and its host's at the anchor it rides. */
export function bindAttachmentTurn(
  keyforms: Readonly<Anime25DTurnKeyforms> | undefined,
  layer: KeyedLayer,
  rest: Float32Array,
  host: KeyedLayer | undefined,
  anchor: { x: number; y: number },
): AttachmentTurn | null {
  const own = bindTurnKeyform(keyforms, layer, rest)
  if (!own) return null
  return {
    own,
    host: host ? bindTurnKeyform(keyforms, host, new Float32Array([anchor.x, anchor.y, anchor.x + 1, anchor.y])) : null,
    anchor: { x: anchor.x, y: anchor.y },
    amount: Number.NaN,
    nod: Number.NaN,
  }
}

/** The host's key at the anchor as a rigid move: how far the anchor went and how far it turned there. */
export interface HostKeyMove {
  x: number
  y: number
  cosine: number
  sine: number
}

export function hostKeyMove(turn: Readonly<AttachmentTurn>, amount: number, nod: number, out: HostKeyMove): HostKeyMove {
  out.x = 0
  out.y = 0
  out.cosine = 1
  out.sine = 0
  if (!turn.host) return out
  turnKeyformOffset(turn.host, 0, amount, keyed, nod)
  out.x = keyed.x
  out.y = keyed.y
  turnKeyformOffset(turn.host, 1, amount, keyed, nod)
  const dx = 1 + keyed.x - out.x
  const dy = keyed.y - out.y
  const length = Math.hypot(dx, dy)
  if (length > 1e-6) {
    out.cosine = dx / length
    out.sine = dy / length
  }
  return out
}

/** The rotation of the host's key at the anchor (radians), which a keyed accessory's vertices take off. */
export function hostKeyRoll(turn: Readonly<AttachmentTurn>, amount: number, nod: number): number {
  hostKeyMove(turn, amount, nod, roll)
  return Math.atan2(roll.sine, roll.cosine)
}

const roll: HostKeyMove = { x: 0, y: 0, cosine: 1, sine: 0 }

/**
 * Takes the host's key at the anchor back off a point. The rigid carry brings
 * the host's whole motion at the anchor, its key included, turn and all; a
 * keyed accessory's own key already holds where it goes, so the carry must not
 * bring the host's key a second time.
 */
export function undoHostKey(
  turn: Readonly<AttachmentTurn>,
  move: Readonly<HostKeyMove>,
  x: number,
  y: number,
  out: { x: number; y: number },
): void {
  const rx = x - turn.anchor.x - move.x
  const ry = y - turn.anchor.y - move.y
  out.x = turn.anchor.x + rx * move.cosine + ry * move.sine
  out.y = turn.anchor.y - rx * move.sine + ry * move.cosine
}

/**
 * Writes an accessory's vertices for the turn: each goes by its own key, with
 * the host's at the anchor taken off, which the rigid carry brings. False when
 * nothing changed since the last frame.
 */
export function deformAttachmentTurn(
  turn: AttachmentTurn,
  amount: number,
  nod: number,
  rest: Float32Array,
  deformed: Float32Array,
): boolean {
  if (amount === turn.amount && nod === turn.nod) return false
  turn.amount = amount
  turn.nod = nod
  hostKeyMove(turn, amount, nod, move)
  for (let i = 0; i < rest.length; i += 2) {
    turnKeyformOffset(turn.own, i / 2, amount, keyed, nod)
    undoHostKey(turn, move, rest[i] + keyed.x, rest[i + 1] + keyed.y, keyed)
    deformed[i] = keyed.x
    deformed[i + 1] = keyed.y
  }
  return true
}

const keyed = { x: 0, y: 0 }
const move: HostKeyMove = { x: 0, y: 0, cosine: 1, sine: 0 }

/** Adds the keyed turn at `amount` to a point of vertex `vertex`. */
export function addKeyedTurn(
  point: { x: number; y: number },
  bound: Readonly<BoundTurnKeyform>,
  vertex: number,
  amount: number,
  nod = 0,
): void {
  turnKeyformOffset(bound, vertex, amount, keyed, nod)
  point.x += keyed.x
  point.y += keyed.y
}

function sampleLattice(lattice: Readonly<Anime25DTurnLattice>, x: number, y: number, out: Float32Array): void {
  const [x0, y0, x1, y1] = lattice.box
  const last = lattice.grid - 1
  const gx = Math.max(0, Math.min(last - 1e-6, ((x - x0) / Math.max(1e-6, x1 - x0)) * last))
  const gy = Math.max(0, Math.min(last - 1e-6, ((y - y0) / Math.max(1e-6, y1 - y0)) * last))
  const i = Math.floor(gx)
  const j = Math.floor(gy)
  const tx = gx - i
  const ty = gy - j
  const values = lattice.back
  const corner = (ci: number, cj: number, c: number) => values[(cj * lattice.grid + ci) * 2 + c]
  for (let c = 0; c < 2; c++) {
    out[c] =
      corner(i, j, c) * (1 - tx) * (1 - ty) +
      corner(i + 1, j, c) * tx * (1 - ty) +
      corner(i, j + 1, c) * (1 - tx) * ty +
      corner(i + 1, j + 1, c) * tx * ty
  }
}

export function isAnime25DTurnKeyforms(value: unknown): value is Anime25DTurnKeyforms {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  return Object.values(value as Record<string, unknown>).every((key) => {
    if (!key || typeof key !== 'object') return false
    const { plus, minus, up, down } = key as Record<string, unknown>
    return isLattice(plus) && isLattice(minus) && (up === undefined || isLattice(up)) && (down === undefined || isLattice(down))
  })
}

function isLattice(value: unknown): value is Anime25DTurnLattice {
  if (!value || typeof value !== 'object') return false
  const { box, grid, back } = value as Record<string, unknown>
  if (!Array.isArray(box) || box.length !== 4 || !box.every(Number.isFinite)) return false
  if (typeof grid !== 'number' || !Number.isInteger(grid) || grid < 2 || grid > 64) return false
  return Array.isArray(back) && back.length === grid * grid * 2 && back.every(Number.isFinite)
}
