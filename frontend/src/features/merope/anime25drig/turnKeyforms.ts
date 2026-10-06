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
 * own key ('front-hair:N') where one was measured, else with the whole front hair.
 */
function turnKeyFor(keyforms: Readonly<Anime25DTurnKeyforms> | undefined, layer: KeyedLayer): Anime25DTurnKey | undefined {
  const family = turnKeyformFamily(layer)
  if (!family || !keyforms) return undefined
  const lock = family === 'front-hair' ? /^front-hair-(\d+)$/.exec(layer.name ?? '') : null
  return (lock ? keyforms[`front-hair:${lock[1]}`] : undefined) ?? keyforms[family]
}

export interface BoundTurnKeyform {
  plus: Float32Array
  minus: Float32Array
  up: Float32Array | null
  down: Float32Array | null
}

/** Each vertex's offset at the full turn either way: where in the turned drawing its rest point went. */
export function bindTurnKeyform(
  keyforms: Readonly<Anime25DTurnKeyforms> | undefined,
  layer: KeyedLayer,
  rest: Float32Array,
): BoundTurnKeyform | null {
  const key = turnKeyFor(keyforms, layer)
  if (!key) return null
  return {
    plus: invertLattice(key.plus, rest),
    minus: invertLattice(key.minus, rest),
    up: key.up && key.down ? invertLattice(key.up, rest) : null,
    down: key.up && key.down ? invertLattice(key.down, rest) : null,
  }
}

/** For each rest point q, the turned point t with t + back(t) = q, as t − q. */
function invertLattice(lattice: Readonly<Anime25DTurnLattice>, rest: Float32Array): Float32Array {
  const out = new Float32Array(rest.length)
  const at = new Float32Array(2)
  for (let i = 0; i < rest.length; i += 2) {
    const qx = rest[i]
    const qy = rest[i + 1]
    let tx = qx
    let ty = qy
    for (let step = 0; step < 30; step++) {
      sampleLattice(lattice, tx, ty, at)
      const nx = qx - at[0]
      const ny = qy - at[1]
      const moved = Math.abs(nx - tx) + Math.abs(ny - ty)
      tx = nx
      ty = ny
      if (moved < 0.01) break
    }
    out[i] = tx - qx
    out[i + 1] = ty - qy
  }
  return out
}

/**
 * The offset at `amount` of the full turn (−1…1) and `nod` of the full nod,
 * for vertex `vertex`. Turn and nod keys add, as Live2D fills a deformer's
 * corner forms from its edge forms.
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
  out.x += nodKey[vertex * 2] * nodShare
  out.y += nodKey[vertex * 2 + 1] * nodShare
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
