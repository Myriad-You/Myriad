import type { Anime25DTurnKey, Anime25DTurnKeyforms, Anime25DTurnLattice } from '../anime25drig/turnKeyforms'
import { isAnime25DTurnKeyforms } from '../anime25drig/turnKeyforms'

/** Keys measured on a decomposition, in its document's pixels. */
export interface DocumentTurnKeyforms {
  canvas: [number, number]
  keyforms: Anime25DTurnKeyforms
}

const keyed = new WeakMap<Blob, DocumentTurnKeyforms>()

/** Remembers the keys that go with a decomposition file until it is imported. */
export function attachTurnKeyforms(file: Blob, keys: DocumentTurnKeyforms): void {
  keyed.set(file, keys)
}

export function turnKeyformsFor(file: Blob): DocumentTurnKeyforms | undefined {
  return keyed.get(file)
}

export function isDocumentTurnKeyforms(value: unknown): value is DocumentTurnKeyforms {
  if (!value || typeof value !== 'object') return false
  const { canvas, keyforms } = value as Record<string, unknown>
  return Array.isArray(canvas) && canvas.length === 2 && canvas.every((v) => Number.isFinite(v) && v > 0) &&
    isAnime25DTurnKeyforms(keyforms)
}

/**
 * The keys in the playback's pixels: the playback frame is the document cut
 * at (frame.x, frame.y) at one scale, so lattices move by the cut and their
 * offsets stay. Undefined when the keys were measured on another document.
 */
export function placeTurnKeyforms(
  keys: DocumentTurnKeyforms,
  document: { width: number; height: number },
  frame: { x: number; y: number },
): Anime25DTurnKeyforms | undefined {
  if (keys.canvas[0] !== document.width || keys.canvas[1] !== document.height) return undefined
  const place = (lattice: Anime25DTurnLattice): Anime25DTurnLattice => ({
    ...lattice,
    box: [lattice.box[0] - frame.x, lattice.box[1] - frame.y, lattice.box[2] - frame.x, lattice.box[3] - frame.y],
  })
  return Object.fromEntries(Object.entries(keys.keyforms).map(([family, key]): [string, Anime25DTurnKey] => [family, {
    plus: place(key.plus),
    minus: place(key.minus),
    ...(key.up && key.down ? { up: place(key.up), down: place(key.down) } : {}),
  }]))
}
