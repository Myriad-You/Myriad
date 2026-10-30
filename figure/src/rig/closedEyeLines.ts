/**
 * A closed eye is lines drawn on the lid: the lashes' arc and the crease.
 * The lid's skin is the face's, under it. A decomposition that draws the
 * closed eye from an open one sometimes paints the lid too, a patch of pink or
 * skin over the crease that matches nothing under it and shows as a smear when
 * she closes her eyes; so a closed eye keeps its dark lines and lets its light
 * fills go. On the drawings measured the lines sit below 96 of 255 in
 * luminance and the fills from 130 up, with almost nothing between.
 */
export const LINE_DARK = 110
export const FILL_LIGHT = 150

/** How much of a pixel of this colour a closed eye keeps: 1 for a line, 0 for a fill. */
export function closedEyeLineShare(red: number, green: number, blue: number): number {
  const luminance = 0.299 * red + 0.587 * green + 0.114 * blue
  const t = Math.max(0, Math.min(1, (FILL_LIGHT - luminance) / (FILL_LIGHT - LINE_DARK)))
  return t * t * (3 - 2 * t)
}

/** A closed eye's pixels (RGBA, straight alpha) with its light fills let go; a new array. */
export function keepClosedEyeLines(data: Readonly<Uint8ClampedArray>): Uint8ClampedArray {
  const out = new Uint8ClampedArray(data)
  for (let i = 0; i < out.length; i += 4) {
    if (out[i + 3] === 0) continue
    out[i + 3] = Math.round(out[i + 3] * closedEyeLineShare(out[i], out[i + 1], out[i + 2]))
  }
  return out
}
