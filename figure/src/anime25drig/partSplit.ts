import type { Anime25DOwnTexture } from './renderer'
import type { Anime25DPart, Anime25DPartProfile, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

/**
 * A hanging piece drawn into a layer it shares (a pendant on the choker, a
 * charm on a garter, an earring in the hair), cut out to swing on its own,
 * as a rigger would give it a layer. The import's vision inventory says where
 * it is (partProfile); here its pixels are told from what is around it by
 * colour, moved to a layer of their own drawn just over the host, and the hole
 * they leave in the host is filled from the host around it. A piece that
 * cannot be told apart cleanly stays as it was.
 */
export interface SplitPart {
  /** The piece's own layer: over its host, with its own pixels. */
  source: Anime25DPlaybackLayer
  pixels: CroppedLayerPixels
  host: Anime25DPlaybackLayer
  /** The host's pixels with the piece taken out and filled. */
  hostPixels: CroppedLayerPixels
}

/** A piece bigger than this share of the canvas is not a hanging piece. */
const MAX_AREA_SHARE = 0.03
/** A pixel this far (sum of channel differences) from the colour around the piece is the piece. */
const PIECE_DISTANCE = 72
/** A piece is at least this share of its box, and at most this share of the opaque pixels looked at. */
const PIECE_SHARE = [0.01, 0.85] as const
/** A piece takes this many rows of what it hangs by along, over its cut. */
const OVERLAP = 4
/** Within this many px of its host's plain cloth, a pixel is the cloth's. */
const CLOTH_GAP = 4
/** Lines this thin (px either side) are outlines, not pieces. */
const OUTLINE = 2
/** A hanging piece has open space on both sides in this share of its rows, over at least this share of its length. */
const SIDE_OPEN = 0.75
const MIN_FREE = 0.4
/** This many rows in a row against its host, and what is above them is the host's. */
const AGAINST_ROWS = 4
/** A string this long (px) or shorter, too thin to keep, still ties what hangs from it on. */
const STRING = 16
/** A part kept is at least this share of the biggest that hangs there. */
const STRUNG_SHARE = 0.2
/** A piece reaches up to within this share of its box of where it hangs from. */
const HANG_GAP = 0.08
/** A hanging piece is at least this tall for its width, and what it hangs by at least this. */
const HANG_ASPECT = 0.8
const SEED_ASPECT = 0.5
/** The inventory's boxes are often off by this much of their size: look that far around. */
const BOX_SLACK = 0.75
/** The window looks this share of its box above where it hangs. */
const ABOVE_ANCHOR = 0.15
/** Where it hangs from may be lower than the inventory says, by these shares of its box. */
const ANCHOR_DROPS = [0, 0.08, 0.16, 0.24, 0.32]
/** Layers a piece may be drawn into. */
const HOST_ROLES = new Set(['front-hair', 'back-hair', 'headwear', 'neckwear', 'neck', 'topwear', 'bottomwear', 'legwear', 'footwear', 'handwear', 'objects'])
/** Layers that go with the head; a piece hung from the head is drawn in one of them, and only such a piece. */
const HEAD_ROLES = new Set(['front-hair', 'back-hair', 'headwear'])

const cutHosts = new WeakMap<Anime25DPlaybackLayer, string>()

/** The layer a piece's own layer was cut out of: what it hangs from. */
export function cutFrom(source: Anime25DPlaybackLayer): string | undefined {
  return cutHosts.get(source)
}

/** A piece's host was itself split: it hangs from the part of it named. */
export function hangCutFrom(source: Anime25DPlaybackLayer, hostName: string): void {
  if (cutHosts.has(source)) cutHosts.set(source, hostName)
}

/** The name of a piece's own layer, which says which part of the inventory it is. */
export function splitLayerName(index: number, part: Readonly<Anime25DPart>): string {
  return `part:${index}:${part.name}`
}

/** The part a layer is, by its name: drawn alone in it, or cut out to it. */
export function partOfLayer(profile: Anime25DPartProfile | undefined, name: string): Anime25DPart | null {
  const cut = /^part:(\d+):/.exec(name)
  if (cut) return profile?.parts[Number(cut[1])] ?? null
  return profile?.parts.find((part) => part.layer === name) ?? null
}

/** Whether a part is a hanging piece to cut out of the layer it shares. */
export function cutsOut(part: Readonly<Anime25DPart>, canvas: { width: number; height: number }): boolean {
  const [x0, y0, x1, y1] = part.box
  return !part.separate && part.motion === 'dangle' && (x1 - x0) * (y1 - y0) <= MAX_AREA_SHARE * canvas.width * canvas.height
}

/** The layer a piece is drawn in: the one the inventory named, or the topmost one opaque at its middle. */
function hostOf(
  part: Readonly<Anime25DPart>,
  layers: readonly Anime25DPlaybackLayer[],
  read: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): Anime25DPlaybackLayer | null {
  if (part.layer) return layers.find((layer) => layer.name === part.layer && HOST_ROLES.has(layer.role)) ?? null
  const cx = (part.box[0] + part.box[2]) / 2
  const cy = (part.box[1] + part.box[3]) / 2
  for (let i = layers.length - 1; i >= 0; i--) {
    const layer = layers[i]
    if (!HOST_ROLES.has(layer.role) || layer.fade) continue
    if (cx < layer.x || cx >= layer.x + layer.w || cy < layer.y || cy >= layer.y + layer.h) continue
    const art = read(layer)
    if (!art) continue
    const px = Math.floor(((cx - layer.x) / layer.w) * art.width)
    const py = Math.floor(((cy - layer.y) / layer.h) * art.height)
    if (art.pixels[(py * art.width + px) * 4 + 3] > 128) return layer
  }
  return null
}

/** Where a piece lies in its host's pixels, before it is cut out. */
interface PieceMask {
  mask: Uint8Array
  x0: number
  y0: number
  w: number
  h: number
  size: number
}

/**
 * Cuts one piece out of its host's pixels. `box` is in the host's pixels.
 * Returns the piece's pixels (as big as it is, at x, y) and the host's, filled; null when the
 * piece is not told apart from what is around it.
 */
export function cutPiece(
  art: Readonly<CroppedLayerPixels>,
  box: readonly [number, number, number, number],
  anchorY = box[1],
): CutPiece | null {
  const found = pieceMask(art, box, anchorY, colourOf(art, box))
  return found ? applyCut(art, found) : null
}

interface CutPiece {
  piece: CroppedLayerPixels
  x: number
  y: number
  size: number
  host: CroppedLayerPixels
}

/** Which of a box's pixels are a piece hanging from row `anchorY`; null when none can be told apart. */
function pieceMask(
  art: Readonly<CroppedLayerPixels>,
  box: readonly [number, number, number, number],
  anchorY: number,
  cloth: readonly number[] | null,
  height = box[3] - box[1],
  /** The piece's own box's area, when `box` is a wider window around it. */
  area = (box[2] - box[0]) * (box[3] - box[1]),
): PieceMask | null {
  // The inventory's boxes are loose: look a little around it.
  const margin = Math.max(2, 0.1 * Math.max(box[2] - box[0], box[3] - box[1]))
  const [x0, y0, x1, y1] = [
    Math.max(0, Math.floor(box[0] - margin)), Math.max(0, Math.floor(box[1] - margin)),
    Math.min(art.width, Math.ceil(box[2] + margin)), Math.min(art.height, Math.ceil(box[3] + margin)),
  ]
  const w = x1 - x0
  const h = y1 - y0
  if (w < 3 || h < 3) return null
  const px = art.pixels
  const at = (x: number, y: number) => (y * art.width + x) * 4
  // The colour around the piece: the median of the opaque pixels on the box's rim.
  const rim: number[][] = [[], [], []]
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      if (y > y0 + 1 && y < y1 - 2 && x > x0 + 1 && x < x1 - 2) continue
      const i = at(x, y)
      if (px[i + 3] < 128) continue
      for (let c = 0; c < 3; c++) rim[c].push(px[i + c])
    }
  }
  const median = (values: number[]) => values.sort((a, b) => a - b)[values.length >> 1] ?? 0
  const around = rim[0].length ? rim.map(median) : null
  const mask = new Uint8Array(w * h)
  const solid = new Uint8Array(w * h)
  const plain = new Uint8Array(w * h)
  let opaque = 0
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = at(x0 + x, y0 + y)
      if (px[i + 3] < 64) continue
      opaque += 1
      solid[y * w + x] = 1
      // Hanging over nothing, all of it is the piece; over the host, what
      // differs from it there and from its cloth as a whole (a box's rim may
      // be mostly a frill, and then a lit edge of the cloth differs from that).
      const differs = (!around || distance(px, i, around) > PIECE_DISTANCE) && (!cloth || distance(px, i, cloth) > PIECE_DISTANCE)
      if (!differs) plain[y * w + x] = 1
      // What is above where it hangs from is what it hangs from.
      else if (y0 + y >= anchorY) mask[y * w + x] = 1
    }
  }
  // Plain colour is the cloth only where it runs on from the cloth: out of the
  // box or above where the piece hangs. An island of it below is a facet of
  // the piece that happens to share the cloth's colour.
  const clothBody = plainCloth(plain, w, h, Math.max(0, Math.ceil(anchorY - y0)))
  for (let k = 0; k < mask.length; k++) {
    if (plain[k] && !clothBody[k] && y0 + Math.floor(k / w) >= anchorY) mask[k] = 1
  }
  // What lies right against the cloth is the cloth's (its lit edge, its seam):
  // it does not tie a piece on.
  const onCloth = morph(clothBody, w, h, CLOTH_GAP, true)
  const loose = mask.map((value, k) => value & (1 - onCloth[k]))
  // What differs and runs on out of the box, to the sides or below, is the
  // host's own: a frill, an outline, a stripe. Where the host's art ends, the
  // piece may end too. A thin outline the piece is drawn across does not
  // carry it off with it: what is that thin is set aside first, and the
  // piece then takes back its own edge.
  const core = morph(morph(loose, w, h, OUTLINE, false), w, h, OUTLINE, true)
  keepEnclosed(core, w, h, { left: x0 > 0, right: x1 < art.width, bottom: y1 < art.height })
  // What hangs from where it hangs, and not what merely lies near it.
  // (Less what it has given back to the cloth and its outline there.)
  const from = anchorY - y0
  if (!keepHanging(core, solid, w, h, from, from + Math.max(3, HANG_GAP * height) + CLOTH_GAP + OUTLINE)) return null
  // The piece is what is drawn there: out from its core over what is not the
  // cloth, as far as was set aside, and its own outline with it.
  const piece = spread(core, mask, w, h, CLOTH_GAP + OUTLINE + 1)
  const outline = morph(core, w, h, OUTLINE + 1, true)
  let count = 0
  for (let k = 0; k < mask.length; k++) {
    mask[k] = (piece[k] | (outline[k] & solid[k])) & (y0 + Math.floor(k / w) >= anchorY ? 1 : 0)
    count += mask[k]
  }
  // All of it differing from what is around it is a misread, unless nothing is around it.
  if (!opaque || count < PIECE_SHARE[0] * area || (around && count > PIECE_SHARE[1] * opaque)) return null
  return { mask, x0, y0, w, h, size: count }
}

/** Takes a found piece out of its host: its pixels, and the host's with the hole filled. */
function applyCut(art: Readonly<CroppedLayerPixels>, found: Readonly<PieceMask>): CutPiece {
  const { mask, x0, y0, w, h } = found
  const px = art.pixels
  const at = (x: number, y: number) => (y * art.width + x) * 4
  // Its soft edge too: opaque pixels next to it.
  const grown = mask.slice()
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (mask[y * w + x] || px[at(x0 + x, y0 + y) + 3] < 16) continue
      if ((x > 0 && mask[y * w + x - 1]) || (x < w - 1 && mask[y * w + x + 1]) || (y > 0 && mask[(y - 1) * w + x]) || (y < h - 1 && mask[(y + 1) * w + x])) {
        grown[y * w + x] = 1
      }
    }
  }
  // Where it was cut from what it hangs by, it takes a few rows of that along
  // (left in the host too): swinging, it covers its cut instead of opening it.
  const lap = new Uint8Array(w * h)
  for (let x = 0; x < w; x++) {
    // Where its column starts; above it, what it was cut from, if anything.
    let start = -1
    for (let y = 0; y < h; y++) {
      if (grown[y * w + x]) {
        start = y
        break
      }
    }
    if (start < 0) continue
    // Only as far as it goes on in its own colour: not a line drawn across it.
    const own = Array.from(px.subarray(at(x0 + x, y0 + start), at(x0 + x, y0 + start) + 3))
    for (let y = start - 1; y >= Math.max(0, start - OVERLAP); y--) {
      const i = at(x0 + x, y0 + y)
      if (grown[y * w + x] || px[i + 3] < 16 || distance(px, i, own) > PIECE_DISTANCE) break
      lap[y * w + x] = 1
    }
  }
  // Its own layer is just as big as it is.
  const bounds = [w, h, 0, 0]
  for (let k = 0; k < grown.length; k++) {
    if (!grown[k] && !lap[k]) continue
    const x = k % w
    const y = (k - x) / w
    bounds[0] = Math.min(bounds[0], x)
    bounds[1] = Math.min(bounds[1], y)
    bounds[2] = Math.max(bounds[2], x + 1)
    bounds[3] = Math.max(bounds[3], y + 1)
  }
  const pw = Math.max(1, bounds[2] - bounds[0])
  const ph = Math.max(1, bounds[3] - bounds[1])
  const piece = new Uint8ClampedArray(pw * ph * 4)
  const host = new Uint8ClampedArray(px)
  const known = new Uint8Array(w * h)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = at(x0 + x, y0 + y)
      if (grown[y * w + x] || lap[y * w + x]) piece.set(px.subarray(i, i + 4), ((y - bounds[1]) * pw + x - bounds[0]) * 4)
      if (grown[y * w + x]) continue
      if (px[i + 3] >= 128) {
        known[y * w + x] = 1
      }
    }
  }
  // The hole is filled from the host around it, ring by ring; where nothing of
  // the host is around (the piece hung over nothing), it is left clear.
  for (let pass = 0; pass < w + h; pass++) {
    let filled = 0
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        if (!grown[y * w + x] || known[y * w + x]) continue
        const sum = [0, 0, 0]
        let n = 0
        for (const [dx, dy] of NEIGHBOURS) {
          const nx = x + dx
          const ny = y + dy
          if (nx < 0 || ny < 0 || nx >= w || ny >= h || known[ny * w + nx] !== 1) continue
          const j = at(x0 + nx, y0 + ny)
          for (let c = 0; c < 3; c++) sum[c] += host[j + c]
          n += 1
        }
        if (n < 2) continue
        const i = at(x0 + x, y0 + y)
        for (let c = 0; c < 3; c++) host[i + c] = sum[c] / n
        host[i + 3] = 255
        known[y * w + x] = 2
        filled += 1
      }
    }
    for (let k = 0; k < known.length; k++) {
      if (known[k] === 2) known[k] = 1
    }
    if (!filled) break
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      if (grown[y * w + x] && !known[y * w + x]) host[at(x0 + x, y0 + y) + 3] = 0
    }
  }
  return { piece: { pixels: piece, width: pw, height: ph }, x: x0 + bounds[0], y: y0 + bounds[1], size: found.size, host: { pixels: host, width: art.width, height: art.height } }
}

const NEIGHBOURS = [[-1, 0], [1, 0], [0, -1], [0, 1]] as const

/** Sum of channel differences between a pixel and a colour. */
function distance(px: Readonly<Uint8ClampedArray>, i: number, colour: readonly number[]): number {
  return Math.abs(px[i] - colour[0]) + Math.abs(px[i + 1] - colour[1]) + Math.abs(px[i + 2] - colour[2])
}

/**
 * A layer's cloth colour: the median of its opaque pixels, sampled, away from
 * `away` (a piece's box, widened by half), so a big piece in a small layer
 * does not make itself the cloth.
 */
function colourOf(art: Readonly<CroppedLayerPixels>, away?: readonly [number, number, number, number]): number[] | null {
  const channels: number[][] = [[], [], []]
  const step = Math.max(1, Math.floor(Math.sqrt((art.width * art.height) / 20000)))
  const [ax0, ay0, ax1, ay1] = away ?? [0, 0, 0, 0]
  const padX = (ax1 - ax0) / 2
  const padY = (ay1 - ay0) / 2
  for (let y = 0; y < art.height; y += step) {
    for (let x = 0; x < art.width; x += step) {
      if (away && x > ax0 - padX && x < ax1 + padX && y > ay0 - padY && y < ay1 + padY) continue
      const i = (y * art.width + x) * 4
      if (art.pixels[i + 3] < 128) continue
      for (let c = 0; c < 3; c++) channels[c].push(art.pixels[i + c])
    }
  }
  if (channels[0].length < 50) return null
  return channels.map((values) => values.sort((a, b) => a - b)[values.length >> 1])
}

/**
 * Keeps, of a w×h mask, the connected parts that hang from row `from`: that
 * reach up to it (by row `reach`, allowing for what was set aside), are drawn out downward by their weight (a flat run is part
 * of the host) and hang in the open on both sides (`solid` is the host's
 * opaque pixels): a pendant hangs free, a frill, a stripe or the lit edge of
 * a leg lies on its cloth. What hangs in the open a few pixels below a kept
 * part, by a string too thin to keep, goes with it. Clears the rest; returns
 * how many parts it kept.
 */
function keepHanging(mask: Uint8Array, solid: Readonly<Uint8Array>, w: number, h: number, from: number, reach: number): number {
  const label = new Int32Array(mask.length)
  const hanging = new Set<number>()
  /** Parts hanging free, by how big they are. */
  const free = new Map<number, number>()
  const members: number[] = []
  let biggest = 0
  let next = 0
  for (let start = 0; start < mask.length; start++) {
    if (!mask[start] || label[start]) continue
    next += 1
    label[start] = next
    members.length = 0
    const queue = [start]
    const bounds = [w, h, -1, -1]
    while (queue.length) {
      const k = queue.pop()!
      members.push(k)
      const x = k % w
      const y = (k - x) / w
      bounds[0] = Math.min(bounds[0], x)
      bounds[1] = Math.min(bounds[1], y)
      bounds[2] = Math.max(bounds[2], x)
      bounds[3] = Math.max(bounds[3], y)
      for (const [dx, dy] of NEIGHBOURS) {
        const nx = x + dx
        const ny = y + dy
        const n = ny * w + nx
        if (nx < 0 || ny < 0 || nx >= w || ny >= h || !mask[n] || label[n]) continue
        label[n] = next
        queue.push(n)
      }
    }
    const freeFrom = hangsFreeFrom(members, solid, w, from)
    if (freeFrom < 0) continue
    // Above where it hangs free it lies against its host, and stays with it.
    bounds[0] = w
    bounds[1] = h
    bounds[2] = -1
    for (const k of members) {
      const x = k % w
      const y = (k - x) / w
      if (y < freeFrom) {
        mask[k] = 0
        label[k] = 0
        continue
      }
      bounds[0] = Math.min(bounds[0], x)
      bounds[1] = Math.min(bounds[1], y)
      bounds[2] = Math.max(bounds[2], x)
    }
    // What it hangs by hangs too: a clasp spread flat across it is the host's.
    const kept = members.filter((k) => mask[k]).length
    if (bounds[3] - bounds[1] < SEED_ASPECT * (bounds[2] - bounds[0])) continue
    free.set(next, kept)
    if (bounds[1] <= reach) {
      hanging.add(next)
      biggest = Math.max(biggest, kept)
    }
  }
  // Not a crumb of its host the cut left loose.
  for (const id of hanging) {
    if ((free.get(id) ?? 0) < STRUNG_SHARE * biggest) hanging.delete(id)
  }
  // What hangs from a kept part by a thin string.
  for (let hop = 0; hop < 3 && hanging.size; hop++) {
    const kept = new Uint8Array(mask.length)
    for (let k = 0; k < mask.length; k++) kept[k] = hanging.has(label[k]) ? 1 : 0
    const near = morph(kept, w, h, STRING, true)
    const before = hanging.size
    for (let k = 0; k < mask.length; k++) {
      if (near[k] && (free.get(label[k]) ?? 0) >= STRUNG_SHARE * biggest) hanging.add(label[k])
    }
    if (hanging.size === before) break
  }
  const bounds = [w, h, -1, -1]
  for (let k = 0; k < mask.length; k++) {
    mask[k] = hanging.has(label[k]) ? 1 : 0
    if (!mask[k]) continue
    const x = k % w
    const y = (k - x) / w
    bounds[0] = Math.min(bounds[0], x)
    bounds[1] = Math.min(bounds[1], y)
    bounds[2] = Math.max(bounds[2], x)
    bounds[3] = Math.max(bounds[3], y)
  }
  // Together it is drawn out downward by its weight; a flat run is part of the host.
  if (!hanging.size || bounds[3] - bounds[1] < HANG_ASPECT * (bounds[2] - bounds[0])) return 0
  return hanging.size
}

/**
 * Where a part starts to hang in the open: the row from which, down to its
 * end, most rows have open space (not its host) a little way out on both
 * sides, past its own outline. Above it, the part lies against its host. -1
 * when it hangs free for too little of itself: a frill, a stripe or the lit
 * edge of a leg lies on its cloth all the way.
 */
function hangsFreeFrom(members: readonly number[], solid: Readonly<Uint8Array>, w: number, from: number): number {
  const left = new Map<number, number>()
  const right = new Map<number, number>()
  for (const k of members) {
    const x = k % w
    const y = (k - x) / w
    if (y < from) continue
    left.set(y, Math.min(left.get(y) ?? x, x))
    right.set(y, Math.max(right.get(y) ?? x, x))
  }
  const rows = [...left.keys()].sort((a, b) => a - b)
  if (!rows.length) return -1
  const openAt = (y: number, x: number, step: -1 | 1) => {
    for (let d = 1; d <= REACH_OUT; d++) {
      const nx = x + step * d
      if (nx < 0 || nx >= w) return false
      if (!solid[y * w + nx]) return true
    }
    return false
  }
  // It hangs free below the last stretch of rows where it lies against its host.
  let start = 0
  let against = 0
  const isOpen = rows.map((y) => openAt(y, left.get(y)!, -1) && openAt(y, right.get(y)!, 1))
  isOpen.forEach((open, i) => {
    against = open ? 0 : against + 1
    if (against >= AGAINST_ROWS) start = i + 1
  })
  const free = isOpen.slice(start)
  const length = rows[rows.length - 1] - rows[0] + 1
  if (!free.length || rows[rows.length - 1] - rows[start] + 1 < MIN_FREE * length) return -1
  return free.filter(Boolean).length >= SIDE_OPEN * free.length ? rows[start] : -1
}

/** How far out (px) past its own outline a piece is looked at for open space. */
const REACH_OUT = 12

/** A w×h mask grown (or shrunk) by a square of radius r. */
function morph(mask: Uint8Array, w: number, h: number, r: number, grow: boolean): Uint8Array {
  // Separable, along rows then along columns, each a running count over the
  // window: as fast for any radius. Outside the box counts as unset.
  const pass = (from: Uint8Array, along: 'x' | 'y') => {
    const to = new Uint8Array(from.length)
    const lines = along === 'x' ? h : w
    const length = along === 'x' ? w : h
    const stride = along === 'x' ? 1 : w
    const full = 2 * r + 1
    for (let line = 0; line < lines; line++) {
      const base = along === 'x' ? line * w : line
      let count = 0
      for (let i = 0; i <= Math.min(r, length - 1); i++) count += from[base + i * stride] ? 1 : 0
      for (let i = 0; i < length; i++) {
        to[base + i * stride] = (grow ? count > 0 : count === full) ? 1 : 0
        if (i - r >= 0 && from[base + (i - r) * stride]) count -= 1
        if (i + r + 1 < length && from[base + (i + r + 1) * stride]) count += 1
      }
    }
    return to
  }
  return pass(pass(mask, 'x'), 'y')
}

/** The plain pixels of a w×h box that run on from its edges or from above row `top`. */
function plainCloth(plain: Readonly<Uint8Array>, w: number, h: number, top: number): Uint8Array {
  const cloth = new Uint8Array(plain.length)
  const queue: number[] = []
  const reach = (k: number) => {
    if (!plain[k] || cloth[k]) return
    cloth[k] = 1
    queue.push(k)
  }
  for (let k = 0; k < plain.length; k++) {
    const x = k % w
    const y = (k - x) / w
    if (y < top || x === 0 || y === 0 || x === w - 1 || y === h - 1) reach(k)
  }
  while (queue.length) {
    const k = queue.pop()!
    const x = k % w
    const y = (k - x) / w
    for (const [dx, dy] of NEIGHBOURS) {
      const nx = x + dx
      const ny = y + dy
      if (nx >= 0 && ny >= 0 && nx < w && ny < h) reach(ny * w + nx)
    }
  }
  return cloth
}

/** From `seed`, over `through`, out to `steps` px: what a piece spreads back over. */
function spread(seed: Readonly<Uint8Array>, through: Readonly<Uint8Array>, w: number, h: number, steps: number): Uint8Array {
  const out = seed.slice()
  let front: number[] = []
  for (let k = 0; k < seed.length; k++) {
    if (seed[k]) front.push(k)
  }
  for (let step = 0; step < steps && front.length; step++) {
    const next: number[] = []
    for (const k of front) {
      const x = k % w
      const y = (k - x) / w
      for (const [dx, dy] of NEIGHBOURS) {
        const nx = x + dx
        const ny = y + dy
        const n = ny * w + nx
        if (nx < 0 || ny < 0 || nx >= w || ny >= h || out[n] || !through[n]) continue
        out[n] = 1
        next.push(n)
      }
    }
    front = next
  }
  return out
}

/** Clears the parts of a w×h mask that reach the given edges; returns how much is left. */
function keepEnclosed(mask: Uint8Array, w: number, h: number, edges: { left: boolean; right: boolean; bottom: boolean }): number {
  const queue: number[] = []
  const clear = (k: number) => {
    if (mask[k] !== 1) return
    mask[k] = 0
    queue.push(k)
  }
  for (let x = 0; x < w && edges.bottom; x++) clear((h - 1) * w + x)
  for (let y = 0; y < h; y++) {
    if (edges.left) clear(y * w)
    if (edges.right) clear(y * w + w - 1)
  }
  while (queue.length) {
    const k = queue.pop()!
    const x = k % w
    const y = (k - x) / w
    for (const [dx, dy] of NEIGHBOURS) {
      const nx = x + dx
      const ny = y + dy
      if (nx >= 0 && ny >= 0 && nx < w && ny < h) clear(ny * w + nx)
    }
  }
  let count = 0
  for (const value of mask) count += value
  return count
}

/**
 * A part's piece where the inventory put it, or near there: its boxes are
 * often off by a good share of their size, so it is looked for in a wider
 * window; and where it hangs from may be a knot whose loops belong to the
 * host, so lower hanging lines are tried too. The line that frees the most of
 * it wins.
 */
function findPiece(art: Readonly<CroppedLayerPixels>, part: Readonly<Anime25DPart>, toX: (x: number) => number, toY: (y: number) => number) {
  const [x0, y0, x1, y1] = [toX(part.box[0]), toY(part.box[1]), toX(part.box[2]), toY(part.box[3])]
  const padX = BOX_SLACK * (x1 - x0)
  const padY = BOX_SLACK * (y1 - y0)
  const anchorY = toY(part.anchor[1])
  // Above where it hangs is only what it hangs from: the window starts just over it.
  const window = [x0 - padX, Math.max(y0 - padY, Math.min(y0, anchorY) - ABOVE_ANCHOR * (y1 - y0)), x1 + padX, y1 + padY] as const
  const cloth = colourOf(art, [x0, y0, x1, y1])
  let best: PieceMask | null = null
  for (const drop of ANCHOR_DROPS) {
    const found = pieceMask(art, window, anchorY + drop * (y1 - y0), cloth, y1 - y0, (x1 - x0) * (y1 - y0))
    if (found && (!best || found.size > best.size)) best = found
  }
  return best && applyCut(art, best)
}

/** The hanging pieces of a package's inventory, cut out of the layers they share. */
export function splitHangingParts(
  layers: readonly Anime25DPlaybackLayer[],
  profile: Anime25DPartProfile | undefined,
  canvas: { width: number; height: number },
  read: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): SplitPart[] {
  const cut: SplitPart[] = []
  const hostPixels = new Map<Anime25DPlaybackLayer, CroppedLayerPixels>()
  profile?.parts.forEach((part, index) => {
    if (!cutsOut(part, canvas)) return
    const host = hostOf(part, layers, read)
    // An inventory that hangs it from the head but finds it in the clothes (or
    // the other way) has mistaken what it is: an "earring" on the collar is a brooch.
    if (!host || (part.bone === 'head') !== HEAD_ROLES.has(host.role)) return
    const art = hostPixels.get(host) ?? read(host)
    if (!art) return
    const sx = art.width / host.w
    const sy = art.height / host.h
    const result = findPiece(art, part, (x: number) => (x - host.x) * sx, (y: number) => (y - host.y) * sy)
    if (!result) return
    hostPixels.set(host, result.host)
    const { x: x0, y: y0 } = result
    const source: Anime25DPlaybackLayer = {
      ...host,
      name: splitLayerName(index, part),
      // Drawn just over its host.
      z: (host.z ?? layers.indexOf(host)) + 0.5,
      role: 'objects',
      side: null,
      phys: null,
      fade: null,
      x: host.x + x0 / sx,
      y: host.y + y0 / sy,
      w: result.piece.width / sx,
      h: result.piece.height / sy,
      atlas: {
        x: host.atlas.x + (x0 / art.width) * host.atlas.w,
        y: host.atlas.y + (y0 / art.height) * host.atlas.h,
        w: (result.piece.width / art.width) * host.atlas.w,
        h: (result.piece.height / art.height) * host.atlas.h,
      },
    }
    cutHosts.set(source, host.name)
    cut.push({ host, hostPixels: result.host, pixels: result.piece, source })
  })
  // A host cut twice keeps both cuts: each later piece reads it with the earlier ones out.
  for (const piece of cut) piece.hostPixels = hostPixels.get(piece.host) ?? piece.hostPixels
  return cut
}

/** A cut piece's pixels as a texture of its own, which its layer draws whole. */
export function createCutTexture(gl: WebGL2RenderingContext, image: Readonly<CroppedLayerPixels>): Anime25DOwnTexture | null {
  const texture = gl.createTexture()
  if (!texture) return null
  // Typed-array uploads are premultiplied here, as the atlas's patches are.
  const pixels = new Uint8Array(image.pixels.length)
  for (let i = 0; i < pixels.length; i += 4) {
    const alpha = image.pixels[i + 3]
    for (let c = 0; c < 3; c++) pixels[i + c] = Math.round((image.pixels[i + c] * alpha) / 255)
    pixels[i + 3] = alpha
  }
  gl.bindTexture(gl.TEXTURE_2D, texture)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, image.width, image.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, pixels)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
  return { texture, uv: [0, 0, 1, 1] }
}
