/**
 * See-through paints the back hair it had to invent, the part behind the face
 * and neck, as a smooth wash of the hair's colours: right for a drawing nobody
 * sees, bare once a head turn uncovers it, where it reads as a blur beside
 * drawn strands. This lays strands into that wash, running the way the drawn
 * strands around it run and as strong as they are, and leaves the wash's
 * colour and shading alone.
 *
 * Only pixels the rest pose hides are touched, so the portrait as drawn does
 * not change by a pixel; the strands fade in from the edge of what is hidden.
 */

/** Luminance spread, over a WINDOW-pixel radius, below which hair reads as an untextured wash. */
const BARE_SPREAD = 2
const WINDOW = 4
/** Half the length of a strand stroke, in pixels. */
const STRAND_REACH = 14
/** Strands fade in over this many pixels from where the hidden wash begins. */
const FEATHER = 8
const OPAQUE = 128

/**
 * Adds strand texture, in place, to opaque pixels that `hidden` marks as
 * covered at rest and that carry no texture of their own. Returns how many
 * pixels changed.
 */
export function textureHiddenHair(
  data: Uint8Array | Uint8ClampedArray,
  width: number,
  height: number,
  hidden: Uint8Array,
  seed = 1,
  tuning: { reach?: number; grain?: number; feather?: number; gain?: number } = {},
): number {
  const reach = tuning.reach ?? STRAND_REACH
  const grain = tuning.grain ?? 1
  const feather = tuning.feather ?? FEATHER
  const gain = tuning.gain ?? 1
  const count = width * height
  const luma = new Float32Array(count)
  const opaque = new Uint8Array(count)
  for (let index = 0; index < count; index += 1) {
    const at = index * 4
    opaque[index] = data[at + 3] >= OPAQUE ? 1 : 0
    luma[index] = 0.299 * data[at] + 0.587 * data[at + 1] + 0.114 * data[at + 2]
  }
  const spread = localSpread(luma, width, height, WINDOW)
  const bare = new Uint8Array(count)
  let bareCount = 0
  for (let index = 0; index < count; index += 1) {
    if (opaque[index] && hidden[index] && spread[index] < BARE_SPREAD) {
      bare[index] = 1
      bareCount += 1
    }
  }
  if (!bareCount) return 0

  // Which way the drawn strands run, and how strongly they stand out, where
  // they are drawn; carried across the wash from there.
  const gradientXX = new Float32Array(count)
  const gradientXY = new Float32Array(count)
  const gradientYY = new Float32Array(count)
  for (let y = 1; y < height - 1; y += 1) {
    for (let x = 1; x < width - 1; x += 1) {
      const index = y * width + x
      if (!opaque[index - 1] || !opaque[index + 1] || !opaque[index - width] || !opaque[index + width]) continue
      const gx = (luma[index + 1] - luma[index - 1]) / 2
      const gy = (luma[index + width] - luma[index - width]) / 2
      gradientXX[index] = gx * gx
      gradientXY[index] = gx * gy
      gradientYY[index] = gy * gy
    }
  }
  for (const field of [gradientXX, gradientXY, gradientYY]) blur(field, width, height, 3, 2)
  const base = luma.slice()
  blur(base, width, height, 3, 3)
  const strength = new Float32Array(count)
  for (let index = 0; index < count; index += 1) {
    strength[index] = (luma[index] - base[index]) ** 2
  }
  blur(strength, width, height, 6, 2)
  const weight = new Float32Array(count)
  for (let index = 0; index < count; index += 1) {
    if (!opaque[index] || spread[index] < BARE_SPREAD * 2) continue
    const xx = gradientXX[index]
    const xy = gradientXY[index]
    const yy = gradientYY[index]
    const sum = xx + yy
    if (sum <= 1e-6) continue
    // Coherence: 1 where every gradient points one way, as across parallel strands.
    weight[index] = Math.sqrt((xx - yy) ** 2 + 4 * xy * xy) / sum
  }
  pushPull([gradientXX, gradientXY, gradientYY, strength], weight, width, height)

  const noise = new Float32Array(count)
  for (let index = 0; index < count; index += 1) noise[index] = hash(index, seed)
  blur(noise, width, height, grain, 1)

  const distance = distanceFrom(bare, width, height, feather)
  const strands = new Float32Array(count)
  let total = 0
  let squares = 0
  for (let index = 0; index < count; index += 1) {
    if (!bare[index]) continue
    const value = streak(noise, gradientXX, gradientXY, gradientYY, width, height, index, reach)
    strands[index] = value
    total += value
    squares += value * value
  }
  const mean = total / bareCount
  const deviation = Math.sqrt(Math.max(1e-9, squares / bareCount - mean * mean))

  let changed = 0
  for (let index = 0; index < count; index += 1) {
    if (!bare[index]) continue
    const fade = smoothstep(distance[index] / feather)
    const delta = ((strands[index] - mean) / deviation) * Math.sqrt(strength[index]) * fade * gain
    const lightness = luma[index]
    if (lightness < 1 || Math.abs(delta) < 0.5) continue
    const scale = Math.max(0, lightness + delta) / lightness
    const at = index * 4
    for (let channel = 0; channel < 3; channel += 1) {
      data[at + channel] = Math.min(255, Math.max(0, Math.round(data[at + channel] * scale)))
    }
    changed += 1
  }
  return changed
}

/** Noise averaged along the strand through a pixel, following the strands as they bend. */
function streak(
  noise: Float32Array,
  xx: Float32Array,
  xy: Float32Array,
  yy: Float32Array,
  width: number,
  height: number,
  start: number,
  reach: number,
): number {
  let total = noise[start]
  let samples = 1
  for (const sign of [1, -1]) {
    let x = start % width
    let y = Math.floor(start / width)
    let previousX = 0
    let previousY = 0
    for (let step = 0; step < reach; step += 1) {
      const index = Math.round(y) * width + Math.round(x)
      // Strands run across the gradient: the tensor's minor axis.
      const angle = 0.5 * Math.atan2(2 * xy[index], xx[index] - yy[index]) + Math.PI / 2
      let dx = Math.cos(angle)
      let dy = Math.sin(angle)
      if (step === 0) {
        dx *= sign
        dy *= sign
      } else if (dx * previousX + dy * previousY < 0) {
        dx = -dx
        dy = -dy
      }
      x += dx
      y += dy
      if (x < 0 || y < 0 || x > width - 1 || y > height - 1) break
      previousX = dx
      previousY = dy
      total += bilinear(noise, width, x, y)
      samples += 1
    }
  }
  return total / samples
}

function localSpread(values: Float32Array, width: number, height: number, radius: number): Float32Array {
  const stride = width + 1
  const sums = new Float64Array(stride * (height + 1))
  const squares = new Float64Array(stride * (height + 1))
  for (let y = 0; y < height; y += 1) {
    let row = 0
    let rowSquares = 0
    for (let x = 0; x < width; x += 1) {
      const value = values[y * width + x]
      row += value
      rowSquares += value * value
      sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row
      squares[(y + 1) * stride + x + 1] = squares[y * stride + x + 1] + rowSquares
    }
  }
  const spread = new Float32Array(width * height)
  for (let y = 0; y < height; y += 1) {
    const top = Math.max(0, y - radius)
    const bottom = Math.min(height, y + radius + 1)
    for (let x = 0; x < width; x += 1) {
      const left = Math.max(0, x - radius)
      const right = Math.min(width, x + radius + 1)
      const area = (bottom - top) * (right - left)
      const a = top * stride + left
      const b = top * stride + right
      const c = bottom * stride + left
      const d = bottom * stride + right
      const mean = (sums[d] - sums[b] - sums[c] + sums[a]) / area
      const meanSquare = (squares[d] - squares[b] - squares[c] + squares[a]) / area
      spread[y * width + x] = Math.sqrt(Math.max(0, meanSquare - mean * mean))
    }
  }
  return spread
}

/** Repeated box blur, in place: `passes` of radius `radius` approximate a Gaussian. */
function blur(values: Float32Array, width: number, height: number, radius: number, passes: number): void {
  const line = new Float32Array(Math.max(width, height))
  for (let pass = 0; pass < passes; pass += 1) {
    for (let y = 0; y < height; y += 1) {
      boxLine(values, y * width, 1, width, radius, line)
    }
    for (let x = 0; x < width; x += 1) {
      boxLine(values, x, width, height, radius, line)
    }
  }
}

function boxLine(
  values: Float32Array,
  offset: number,
  stride: number,
  length: number,
  radius: number,
  line: Float32Array,
): void {
  for (let index = 0; index < length; index += 1) line[index] = values[offset + index * stride]
  let sum = 0
  let samples = 0
  for (let index = 0; index < Math.min(length, radius); index += 1) {
    sum += line[index]
    samples += 1
  }
  for (let index = 0; index < length; index += 1) {
    const enter = index + radius
    if (enter < length) {
      sum += line[enter]
      samples += 1
    }
    const leave = index - radius - 1
    if (leave >= 0) {
      sum -= line[leave]
      samples -= 1
    }
    values[offset + index * stride] = sum / samples
  }
}

/**
 * Fills every field where `weight` is zero from the weighted values around it
 * (push-pull: average down a pyramid, then fill each level from the coarser
 * one). Fields are replaced by their filled values everywhere.
 */
function pushPull(fields: Float32Array[], weight: Float32Array, width: number, height: number): void {
  const levels: { width: number; height: number; weight: Float32Array; fields: Float32Array[] }[] = []
  let level = {
    width,
    height,
    weight: weight.slice(),
    fields: fields.map((field) => field.map((value, index) => value * weight[index])),
  }
  levels.push(level)
  while (level.width > 1 || level.height > 1) {
    const nextWidth = Math.max(1, Math.ceil(level.width / 2))
    const nextHeight = Math.max(1, Math.ceil(level.height / 2))
    const next = {
      width: nextWidth,
      height: nextHeight,
      weight: new Float32Array(nextWidth * nextHeight),
      fields: fields.map(() => new Float32Array(nextWidth * nextHeight)),
    }
    for (let y = 0; y < level.height; y += 1) {
      for (let x = 0; x < level.width; x += 1) {
        const from = y * level.width + x
        const to = (y >> 1) * nextWidth + (x >> 1)
        next.weight[to] += level.weight[from]
        for (let field = 0; field < fields.length; field += 1) next.fields[field][to] += level.fields[field][from]
      }
    }
    // Weighted sums stay sums; the weight saturates so a full cell is trusted as it is.
    for (let index = 0; index < next.weight.length; index += 1) {
      const sum = next.weight[index]
      if (sum <= 1) continue
      for (let field = 0; field < fields.length; field += 1) next.fields[field][index] /= sum
      next.weight[index] = 1
    }
    levels.push(next)
    level = next
  }
  for (let depth = levels.length - 2; depth >= 0; depth -= 1) {
    const fine = levels[depth]
    const coarse = levels[depth + 1]
    for (let y = 0; y < fine.height; y += 1) {
      for (let x = 0; x < fine.width; x += 1) {
        const index = y * fine.width + x
        const known = Math.min(1, fine.weight[index])
        const from = (y >> 1) * coarse.width + (x >> 1)
        for (let field = 0; field < fields.length; field += 1) {
          const own = fine.weight[index] > 0 ? fine.fields[field][index] / Math.max(1, fine.weight[index]) : 0
          fine.fields[field][index] = own * known + coarse.fields[field][from] * (1 - known)
        }
        fine.weight[index] = 1
      }
    }
  }
  for (let field = 0; field < fields.length; field += 1) fields[field].set(levels[0].fields[field])
}

/** Chessboard distance from each marked pixel to the nearest unmarked one, capped at `limit`. */
function distanceFrom(marked: Uint8Array, width: number, height: number, limit: number): Float32Array {
  const distance = new Float32Array(width * height)
  for (let index = 0; index < distance.length; index += 1) distance[index] = marked[index] ? limit : 0
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const index = y * width + x
      if (!distance[index]) continue
      if (x > 0) distance[index] = Math.min(distance[index], distance[index - 1] + 1)
      if (y > 0) distance[index] = Math.min(distance[index], distance[index - width] + 1)
      if (x === 0 || y === 0) distance[index] = Math.min(distance[index], 1)
    }
  }
  for (let y = height - 1; y >= 0; y -= 1) {
    for (let x = width - 1; x >= 0; x -= 1) {
      const index = y * width + x
      if (!distance[index]) continue
      if (x < width - 1) distance[index] = Math.min(distance[index], distance[index + 1] + 1)
      if (y < height - 1) distance[index] = Math.min(distance[index], distance[index + width] + 1)
      if (x === width - 1 || y === height - 1) distance[index] = Math.min(distance[index], 1)
    }
  }
  return distance
}

function bilinear(values: Float32Array, width: number, x: number, y: number): number {
  const left = Math.floor(x)
  const top = Math.floor(y)
  const right = Math.min(width - 1, left + 1)
  const bottom = Math.min(values.length / width - 1, top + 1)
  const tx = x - left
  const ty = y - top
  const upper = values[top * width + left] * (1 - tx) + values[top * width + right] * tx
  const lower = values[bottom * width + left] * (1 - tx) + values[bottom * width + right] * tx
  return upper * (1 - ty) + lower * ty
}

/** Deterministic noise in [−1, 1] per pixel. */
function hash(index: number, seed: number): number {
  let value = (index ^ Math.imul(seed, 0x9E3779B9)) >>> 0
  value = Math.imul(value ^ (value >>> 16), 0x85EBCA6B) >>> 0
  value = Math.imul(value ^ (value >>> 13), 0xC2B2AE35) >>> 0
  value = (value ^ (value >>> 16)) >>> 0
  return (value / 0xFFFFFFFF) * 2 - 1
}

function smoothstep(value: number): number {
  const bounded = Math.max(0, Math.min(1, value))
  return bounded * bounded * (3 - 2 * bounded)
}
