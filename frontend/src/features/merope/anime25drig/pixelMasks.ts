/** Masks over a drawing's pixels, one byte per pixel, row by row. */

/** Chessboard dilation by `radius` pixels, as a separable max over rows then columns. */
export function dilate(mask: Uint8Array, width: number, height: number, radius: number): Uint8Array {
  return spread(mask, width, height, radius, 1)
}

/** Chessboard erosion by `radius` pixels. */
export function erode(mask: Uint8Array, width: number, height: number, radius: number): Uint8Array {
  return spread(mask, width, height, radius, 0)
}

function spread(mask: Uint8Array, width: number, height: number, radius: number, value: number): Uint8Array {
  const rows = new Uint8Array(mask.length)
  for (let y = 0; y < height; y++) {
    let last = -Infinity
    for (let x = 0; x < width; x++) {
      if (mask[y * width + x] === value) last = x
      rows[y * width + x] = x - last <= radius ? value : 1 - value
    }
    last = Infinity
    for (let x = width - 1; x >= 0; x--) {
      if (mask[y * width + x] === value) last = x
      if (last - x <= radius) rows[y * width + x] = value
    }
  }
  const out = new Uint8Array(mask.length)
  for (let x = 0; x < width; x++) {
    let last = -Infinity
    for (let y = 0; y < height; y++) {
      if (rows[y * width + x] === value) last = y
      out[y * width + x] = y - last <= radius ? value : 1 - value
    }
    last = Infinity
    for (let y = height - 1; y >= 0; y--) {
      if (rows[y * width + x] === value) last = y
      if (last - y <= radius) out[y * width + x] = value
    }
  }
  return out
}

/** Pixels from the nearest pixel outside `mask`, two-pass chamfer. */
export function insideDistance(mask: Uint8Array, width: number, height: number): Float32Array {
  const far = width + height
  const distance = new Float32Array(mask.length)
  for (let p = 0; p < mask.length; p++) distance[p] = mask[p] ? far : 0
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const p = y * width + x
      if (!mask[p]) continue
      if (x > 0) distance[p] = Math.min(distance[p], distance[p - 1] + 1)
      if (y > 0) distance[p] = Math.min(distance[p], distance[p - width] + 1)
    }
  }
  for (let y = height - 1; y >= 0; y--) {
    for (let x = width - 1; x >= 0; x--) {
      const p = y * width + x
      if (!mask[p]) continue
      if (x < width - 1) distance[p] = Math.min(distance[p], distance[p + 1] + 1)
      if (y < height - 1) distance[p] = Math.min(distance[p], distance[p + width] + 1)
    }
  }
  return distance
}
