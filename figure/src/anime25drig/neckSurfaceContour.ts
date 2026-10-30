import type { CroppedLayerPixels } from './webglRuntime'

/** Fixed-size uniforms, measured once in rest space rather than per frame. */
export const NECK_SURFACE_COLUMNS = 16

export interface NeckSurfaceContour {
  left: number
  right: number
  bands: Float32Array
}

export interface PixelBounds {
  left: number
  top: number
  right: number
  bottom: number
}

export function opaquePixelBounds(
  image: CroppedLayerPixels,
): PixelBounds | null {
  let left = image.width
  let right = 0
  let top = image.height
  let bottom = 0
  for (let y = 0; y < image.height; y++) {
    for (let x = 0; x < image.width; x++) {
      if (image.pixels[(y * image.width + x) * 4 + 3] < 16) continue
      left = Math.min(left, x)
      right = Math.max(right, x + 1)
      top = Math.min(top, y)
      bottom = Math.max(bottom, y + 1)
    }
  }
  return right > left && bottom > top ? { left, right, top, bottom } : null
}

export function buildNeckSurfaceContour(
  width: number,
  height: number,
  bounds: PixelBounds,
  agreement: Uint8Array,
  start: number,
  end: number,
): NeckSurfaceContour {
  const bands = new Float32Array(NECK_SURFACE_COLUMNS * 2)
  const radius = Math.max(1, Math.round((bounds.right - bounds.left) * 0.06))
  const minimumRun = Math.max(3, Math.ceil((bounds.bottom - bounds.top) * 0.04))
  for (let column = 0; column < NECK_SURFACE_COLUMNS; column++) {
    const center =
      bounds.left +
      (column / (NECK_SURFACE_COLUMNS - 1)) * (bounds.right - bounds.left - 1)
    const left = Math.max(bounds.left, Math.floor(center - radius))
    const right = Math.min(bounds.right, Math.ceil(center + radius + 1))
    let run = 0
    let bestStart = start
    let bestEnd = end
    let bestLength = 0
    for (let y = start; y <= end; y++) {
      let valid = 0
      let matches = 0
      for (let x = left; x < right; x++) {
        const value = agreement[y * width + x]
        if (value === 0) continue
        valid++
        if (value === 2) matches++
      }
      run =
        valid >= (right - left) * 0.5 && matches / valid >= 0.85 ? run + 1 : 0
      if (run >= minimumRun && run > bestLength) {
        bestLength = run
        bestStart = y - run + 1
        bestEnd = y
      }
    }
    bands[column * 2] = (bestStart + 0.5) / height
    bands[column * 2 + 1] = (bestEnd + 0.5) / height
  }
  // Two passes remove local notches, not the slope.
  for (let pass = 0; pass < 2; pass++) {
    const previous = bands.slice()
    for (let column = 1; column < NECK_SURFACE_COLUMNS - 1; column++) {
      for (let edge = 0; edge < 2; edge++) {
        const i = column * 2 + edge
        bands[i] = (previous[i - 2] + 2 * previous[i] + previous[i + 2]) / 4
      }
    }
  }
  return {
    left: (bounds.left + 0.5) / width,
    right: (bounds.right - 0.5) / width,
    bands,
  }
}
