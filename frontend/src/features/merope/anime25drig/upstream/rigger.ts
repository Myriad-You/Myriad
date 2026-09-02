/*!
 * Behavioral TypeScript port of Anime2.5DRig `lib/rigger.js` at
 * d48825867acd081de22b0e7b5585bb562288796d.
 * Copyright (c) 2026 hakoniwa
 * SPDX-License-Identifier: MIT
 *
 * This module intentionally preserves upstream thresholds, mutation, warning
 * text, array order, typed-array rounding, and edge behavior. Myriad-specific
 * improvements belong after this boundary, not inside the compatibility port.
 */
import type {
  UpstreamCleanStats,
  UpstreamComponentLabels,
  UpstreamEyeAnchor,
  UpstreamGenericParts,
  UpstreamHairStrand,
  UpstreamLayerFade,
  UpstreamLayerGroup,
  UpstreamLayerPhysics,
  UpstreamLayerSide,
  UpstreamPeak,
  UpstreamPixelImage,
  UpstreamPsd,
  UpstreamPsdLayer,
  UpstreamRgbaImage,
  UpstreamRig,
  UpstreamRiggerApi,
  UpstreamRiggerOptions,
  UpstreamRigLayer,
} from './types'

interface Slot {
  depth: number
  group: UpstreamLayerGroup
  phys?: Exclude<UpstreamLayerPhysics, null>
  fade?: Exclude<UpstreamLayerFade, null>
  split?: boolean
}

interface Bounds {
  x0: number
  y0: number
  x1: number
  y1: number
}

interface Center {
  cx: number
  cy: number
}

interface Entry {
  name: string
  layer: UpstreamPsdLayer & { imageData: UpstreamPixelImage }
  alpha: Uint8Array
}

const SLOTS: Readonly<Record<string, Slot>> = {
  'back hair': { depth: 0.55, group: 'head', phys: 'hair' },
  bottomwear: { depth: 0.88, group: 'body' },
  neck: { depth: 0.95, group: 'body' },
  topwear: { depth: 0.9, group: 'body' },
  handwear: { depth: 0.86, group: 'body' },
  earwear: { depth: 0.97, group: 'head' },
  ears: { depth: 0.96, group: 'head' },
  face: { depth: 1, group: 'head' },
  facedetail: { depth: 1.02, group: 'head' },
  headwear: { depth: 1.2, group: 'head' },
  mouth_close: { depth: 1.08, group: 'head', fade: 'mouthClose' },
  mouth_open: { depth: 1.08, group: 'head', fade: 'mouthOpen' },
  nose: { depth: 1.15, group: 'head' },
  eyewhite: { depth: 1.06, group: 'head', split: true, fade: 'eyeOpen' },
  eyebrow: { depth: 1.14, group: 'head', split: true },
  irides: { depth: 1.08, group: 'head', split: true, fade: 'eyeOpen' },
  eyelash: { depth: 1.12, group: 'head', split: true, fade: 'eyeOpen' },
  eye_close: { depth: 1.12, group: 'head', split: true, fade: 'eyeClose' },
  'front hair': { depth: 1.28, group: 'head', phys: 'hair' },
}

export function normName(value: string): string {
  let name = (value || '')
    .normalize('NFKC')
    .trim()
    .replace(/ のコピー\s*\d*$/, '')
    .toLowerCase()
  if (name === 'eyelash_c') name = 'eye_close'
  if (name === 'mouth_c') name = 'mouth_close'
  if (name === 'mouth' || /^mouth[ _-]?\d+$/.test(name)) name = 'mouth_open'
  if (name === 'レイヤー 1') name = 'facedetail'
  return name
}

export function baseName(value: string): string {
  return value.replace(/_\d+$/, '')
}

function fullAlphaOf(
  layer: UpstreamPsdLayer & { imageData: UpstreamPixelImage },
  width: number,
  height: number,
): Uint8Array {
  const alpha = new Uint8Array(width * height)
  const image = layer.imageData
  const layerWidth = image.width
  const layerHeight = image.height
  const layerX = layer.left! | 0
  const layerY = layer.top! | 0
  const data = image.data
  for (let y = 0; y < layerHeight; y += 1) {
    const canvasY = y + layerY
    if (canvasY < 0 || canvasY >= height) continue
    const rowOffset = canvasY * width
    const layerOffset = y * layerWidth
    for (let x = 0; x < layerWidth; x += 1) {
      const canvasX = x + layerX
      if (canvasX < 0 || canvasX >= width) continue
      alpha[rowOffset + canvasX] = data[(layerOffset + x) * 4 + 3]
    }
  }
  return alpha
}

export function labelComponents(
  alpha: Uint8Array,
  width: number,
  height: number,
  threshold: number,
): UpstreamComponentLabels {
  const labels = new Int32Array(width * height)
  const sizes = [0]
  const sumX = [0]
  let count = 0
  const stack = new Int32Array(width * height)
  for (let start = 0; start < width * height; start += 1) {
    if (labels[start] || alpha[start] <= threshold) continue
    count += 1
    let stackPointer = 0
    stack[stackPointer++] = start
    labels[start] = count
    let size = 0
    let xSum = 0
    while (stackPointer) {
      const point = stack[--stackPointer]
      size += 1
      xSum += point % width
      const x = point % width
      const y = (point / width) | 0
      if (
        x > 0 &&
        !labels[point - 1] &&
        alpha[point - 1] > threshold
      ) {
        labels[point - 1] = count
        stack[stackPointer++] = point - 1
      }
      if (
        x < width - 1 &&
        !labels[point + 1] &&
        alpha[point + 1] > threshold
      ) {
        labels[point + 1] = count
        stack[stackPointer++] = point + 1
      }
      if (
        y > 0 &&
        !labels[point - width] &&
        alpha[point - width] > threshold
      ) {
        labels[point - width] = count
        stack[stackPointer++] = point - width
      }
      if (
        y < height - 1 &&
        !labels[point + width] &&
        alpha[point + width] > threshold
      ) {
        labels[point + width] = count
        stack[stackPointer++] = point + width
      }
    }
    sizes.push(size)
    sumX.push(xSum)
  }
  return { lab: labels, count, sizes, sumX }
}

function dilate(
  mask: Uint8Array,
  width: number,
  height: number,
  radius: number,
): Uint8Array {
  const temporary = new Uint8Array(width * height)
  const output = new Uint8Array(width * height)
  for (let y = 0; y < height; y += 1) {
    const offset = y * width
    for (let x = 0; x < width; x += 1) {
      let value = 0
      for (let k = -radius; k <= radius; k += 1) {
        const candidateX = x + k
        if (
          candidateX >= 0 &&
          candidateX < width &&
          mask[offset + candidateX]
        ) {
          value = 1
          break
        }
      }
      temporary[offset + x] = value
    }
  }
  for (let x = 0; x < width; x += 1) {
    for (let y = 0; y < height; y += 1) {
      let value = 0
      for (let k = -radius; k <= radius; k += 1) {
        const candidateY = y + k
        if (
          candidateY >= 0 &&
          candidateY < height &&
          temporary[candidateY * width + x]
        ) {
          value = 1
          break
        }
      }
      output[y * width + x] = value
    }
  }
  return output
}

export function cleanAlpha(
  alpha: Uint8Array,
  width: number,
  height: number,
  minPixels: number,
): Uint8Array {
  const components = labelComponents(alpha, width, height, 16)
  if (!components.count) return alpha
  const keptComponents = new Uint8Array(components.count + 1)
  let any = false
  for (let index = 1; index <= components.count; index += 1) {
    if (components.sizes[index] >= minPixels) {
      keptComponents[index] = 1
      any = true
    }
  }
  if (!any) return alpha
  let mask: Uint8Array = new Uint8Array(width * height)
  for (let index = 0; index < width * height; index += 1) {
    if (keptComponents[components.lab[index]]) mask[index] = 1
  }
  mask = dilate(mask, width, height, 3)
  for (let index = 0; index < width * height; index += 1) {
    if (!mask[index]) alpha[index] = 0
  }
  return alpha
}

function splitSides(
  alpha: Uint8Array,
  width: number,
  height: number,
  faceCenterX: number,
): { l?: Uint8Array; r?: Uint8Array } {
  const components = labelComponents(alpha, width, height, 16)
  const sides = new Uint8Array(components.count + 1)
  for (let component = 1; component <= components.count; component += 1) {
    if (components.sizes[component] < 20) continue
    sides[component] =
      components.sumX[component] / components.sizes[component] < faceCenterX
        ? 1
        : 2
  }
  const left = new Uint8Array(width * height)
  const right = new Uint8Array(width * height)
  for (let index = 0; index < width * height; index += 1) {
    if (sides[components.lab[index]] === 1) left[index] = 1
    else if (sides[components.lab[index]] === 2) right[index] = 1
  }
  const result: { l?: Uint8Array; r?: Uint8Array } = {}
  let leftCount = 0
  let rightCount = 0
  for (let index = 0; index < width * height; index += 1) {
    leftCount += left[index]
    rightCount += right[index]
  }
  if (leftCount) result.l = dilate(left, width, height, 3)
  if (rightCount) result.r = dilate(right, width, height, 3)
  return result
}

function bboxOf(
  alpha: Uint8Array,
  width: number,
  height: number,
  threshold: number,
): Bounds | null {
  let x0 = width
  let y0 = height
  let x1 = -1
  let y1 = -1
  for (let y = 0; y < height; y += 1) {
    const offset = y * width
    for (let x = 0; x < width; x += 1) {
      if (alpha[offset + x] <= threshold) continue
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
  }
  return x1 < 0 ? null : { x0, y0, x1, y1 }
}

function centroidOf(
  alpha: Uint8Array,
  width: number,
  height: number,
): Center | null {
  let sumX = 0
  let sumY = 0
  let alphaSum = 0
  for (let y = 0; y < height; y += 1) {
    const offset = y * width
    for (let x = 0; x < width; x += 1) {
      const value = alpha[offset + x]
      if (!value) continue
      sumX += x * value
      sumY += y * value
      alphaSum += value
    }
  }
  return alphaSum
    ? { cx: sumX / alphaSum, cy: sumY / alphaSum }
    : null
}

function resampleRgba(
  source: UpstreamRgbaImage,
  targetWidth: number,
  targetHeight: number,
): Uint8ClampedArray {
  const output = new Uint8ClampedArray(targetWidth * targetHeight * 4)
  const sourceWidth = source.width
  const sourceHeight = source.height
  const data = source.data
  for (let y = 0; y < targetHeight; y += 1) {
    const sourceY = ((y + 0.5) * sourceHeight) / targetHeight - 0.5
    const y0 = Math.max(0, Math.floor(sourceY))
    const y1 = Math.min(sourceHeight - 1, y0 + 1)
    const fractionY = sourceY - y0
    for (let x = 0; x < targetWidth; x += 1) {
      const sourceX = ((x + 0.5) * sourceWidth) / targetWidth - 0.5
      const x0 = Math.max(0, Math.floor(sourceX))
      const x1 = Math.min(sourceWidth - 1, x0 + 1)
      const fractionX = sourceX - x0
      const outputOffset = (y * targetWidth + x) * 4
      for (let channel = 0; channel < 4; channel += 1) {
        const value00 = data[(y0 * sourceWidth + x0) * 4 + channel]
        const value01 = data[(y0 * sourceWidth + x1) * 4 + channel]
        const value10 = data[(y1 * sourceWidth + x0) * 4 + channel]
        const value11 = data[(y1 * sourceWidth + x1) * 4 + channel]
        output[outputOffset + channel] =
          value00 * (1 - fractionX) * (1 - fractionY) +
          value01 * fractionX * (1 - fractionY) +
          value10 * (1 - fractionX) * fractionY +
          value11 * fractionX * fractionY
      }
    }
  }
  return output
}

function meanColorOfImage(
  imageData: Pick<UpstreamRgbaImage, 'data'>,
  darkWeight: boolean,
): [number, number, number] | null {
  const data = imageData.data
  let red = 0
  let green = 0
  let blue = 0
  let weightSum = 0
  for (let index = 0; index < data.length; index += 4) {
    const alpha = data[index + 3]
    if (alpha < 24) continue
    let weight = alpha
    if (darkWeight) {
      const luminance = (data[index] + data[index + 1] + data[index + 2]) / 3
      weight = alpha * (1 - luminance / 255) ** 2
    }
    red += data[index] * weight
    green += data[index + 1] * weight
    blue += data[index + 2] * weight
    weightSum += weight
  }
  return weightSum
    ? [red / weightSum, green / weightSum, blue / weightSum]
    : null
}

function recolorTo(
  data: Uint8ClampedArray,
  target: [number, number, number],
): void {
  let luminanceSum = 0
  let count = 0
  for (let index = 0; index < data.length; index += 4) {
    if (data[index + 3] > 24) {
      luminanceSum += (data[index] + data[index + 1] + data[index + 2]) / 3
      count += 1
    }
  }
  if (!count) return
  const mean = Math.max(8, luminanceSum / count)
  for (let index = 0; index < data.length; index += 4) {
    if (!data[index + 3]) continue
    const factor = Math.min(
      2.2,
      (data[index] + data[index + 1] + data[index + 2]) / 3 / mean,
    )
    data[index] = target[0] * factor
    data[index + 1] = target[1] * factor
    data[index + 2] = target[2] * factor
  }
}

function synthPart(
  name: string,
  genericImage: UpstreamRgbaImage,
  targetWidth: number,
  centerX: number,
  anchorY: number,
  verticalAlignment: number,
  tint: [number, number, number] | null,
  slot: Slot,
  side: UpstreamLayerSide,
): UpstreamRigLayer {
  const scale = targetWidth / genericImage.width
  const width = Math.max(2, Math.round(targetWidth))
  const height = Math.max(2, Math.round(genericImage.height * scale))
  const data = resampleRgba(genericImage, width, height)
  if (tint) recolorTo(data, tint)
  return {
    name,
    x: Math.round(centerX - width / 2),
    y: Math.round(anchorY - verticalAlignment * height),
    w: width,
    h: height,
    z: 0,
    depth: slot.depth,
    group: 'head',
    phys: null,
    fade: slot.fade || null,
    side: side || null,
    strands: null,
    synthetic: true,
    img: { width, height, data },
  }
}

function lastIndexWhere<T>(values: T[], predicate: (value: T) => boolean): number {
  for (let index = values.length - 1; index >= 0; index -= 1) {
    if (predicate(values[index])) return index
  }
  return -1
}

function trimImage(
  imageData: UpstreamRgbaImage,
  threshold = 8,
): UpstreamRgbaImage | null {
  // Upstream uses `thr = thr || 8`; retain that behavior for zero/NaN callers.
  threshold = threshold || 8
  const width = imageData.width
  const height = imageData.height
  const data = imageData.data
  let x0 = width
  let y0 = height
  let x1 = -1
  let y1 = -1
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (data[(y * width + x) * 4 + 3] <= threshold) continue
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
  }
  if (x1 < 0) return null
  const trimmedWidth = x1 - x0 + 1
  const trimmedHeight = y1 - y0 + 1
  const output = new Uint8ClampedArray(trimmedWidth * trimmedHeight * 4)
  for (let y = 0; y < trimmedHeight; y += 1) {
    output.set(
      data.subarray(
        ((y + y0) * width + x0) * 4,
        ((y + y0) * width + x0 + trimmedWidth) * 4,
      ),
      y * trimmedWidth * 4,
    )
  }
  return { width: trimmedWidth, height: trimmedHeight, data: output }
}

export function flattenPsdToImg(psd: UpstreamPsd): UpstreamRgbaImage | null {
  const width = psd.width
  const height = psd.height
  const buffer = new Uint8ClampedArray(width * height * 4)
  for (const child of psd.children || []) {
    if (!child.imageData) continue
    const imageData = child.imageData
    const layerWidth = imageData.width
    const layerHeight = imageData.height
    const layerX = child.left! | 0
    const layerY = child.top! | 0
    const data = imageData.data
    for (let y = 0; y < layerHeight; y += 1) {
      const canvasY = y + layerY
      if (canvasY < 0 || canvasY >= height) continue
      for (let x = 0; x < layerWidth; x += 1) {
        const canvasX = x + layerX
        if (canvasX < 0 || canvasX >= width) continue
        const sourceIndex = (y * layerWidth + x) * 4
        const destinationIndex = (canvasY * width + canvasX) * 4
        const alpha = data[sourceIndex + 3] / 255
        if (!alpha) continue
        for (let channel = 0; channel < 3; channel += 1) {
          buffer[destinationIndex + channel] =
            data[sourceIndex + channel] * alpha +
            buffer[destinationIndex + channel] * (1 - alpha)
        }
        buffer[destinationIndex + 3] = Math.min(
          255,
          data[sourceIndex + 3] +
            buffer[destinationIndex + 3] * (1 - alpha),
        )
      }
    }
  }
  return trimImage({ width, height, data: buffer })
}

export function splitImgLR(
  imageData: UpstreamRgbaImage,
): { l: UpstreamRgbaImage; r: UpstreamRgbaImage } | null {
  const width = imageData.width
  const height = imageData.height
  const data = imageData.data
  const columns = new Uint8Array(width)
  for (let x = 0; x < width; x += 1) {
    for (let y = 0; y < height; y += 1) {
      if (data[(y * width + x) * 4 + 3] > 16) {
        columns[x] = 1
        break
      }
    }
  }
  let best: [number, number] | null = null
  let start = -1
  for (let x = 0; x < width; x += 1) {
    if (!columns[x]) {
      if (start < 0) start = x
    } else {
      if (start > 0 && (!best || x - start > best[1] - best[0])) {
        best = [start, x]
      }
      start = -1
    }
  }
  if (!best) return null
  const middle = (best[0] + best[1]) >> 1
  const cut = (from: number, to: number): UpstreamRgbaImage | null => {
    const cutWidth = to - from
    const output = new Uint8ClampedArray(cutWidth * height * 4)
    for (let y = 0; y < height; y += 1) {
      output.set(
        data.subarray(
          (y * width + from) * 4,
          (y * width + to) * 4,
        ),
        y * cutWidth * 4,
      )
    }
    return trimImage({ width: cutWidth, height, data: output })
  }
  const left = cut(0, middle)
  const right = cut(middle, width)
  return left && right ? { l: left, r: right } : null
}

export function findPeaks(
  values: ArrayLike<number>,
  minDistance: number,
  minProminence: number,
): UpstreamPeak[] {
  const candidates: number[] = []
  for (let index = 1; index < values.length - 1; index += 1) {
    if (
      values[index] > values[index - 1] &&
      values[index] >= values[index + 1]
    ) {
      candidates.push(index)
    }
  }
  const peaks: UpstreamPeak[] = []
  for (const candidate of candidates) {
    let leftMinimum = values[candidate]
    let rightMinimum = values[candidate]
    for (let index = candidate - 1; index >= 0; index -= 1) {
      if (values[index] > values[candidate]) break
      if (values[index] < leftMinimum) leftMinimum = values[index]
    }
    for (let index = candidate + 1; index < values.length; index += 1) {
      if (values[index] > values[candidate]) break
      if (values[index] < rightMinimum) rightMinimum = values[index]
    }
    const prominence =
      values[candidate] - Math.max(leftMinimum, rightMinimum)
    if (prominence >= minProminence) {
      peaks.push({ x: candidate, prom: prominence })
    }
  }
  peaks.sort((left, right) => right.prom - left.prom)
  const kept: UpstreamPeak[] = []
  for (const peak of peaks) {
    let allowed = true
    for (const existing of kept) {
      if (Math.abs(existing.x - peak.x) < minDistance) {
        allowed = false
        break
      }
    }
    if (allowed) kept.push(peak)
  }
  return kept
}

export function detectStrands(
  alpha: Uint8Array,
  width: number,
  height: number,
  minSeparation: number,
  wanted: number,
): UpstreamHairStrand[] {
  const bottom = new Float32Array(width)
  const top = new Float32Array(width)
  let minX = width
  let maxX = -1
  for (let x = 0; x < width; x += 1) {
    top[x] = -1
    bottom[x] = 0
    for (let y = 0; y < height; y += 1) {
      if (alpha[y * width + x] > 16) {
        top[x] = y
        break
      }
    }
    if (top[x] < 0) continue
    for (let y = height - 1; y >= 0; y -= 1) {
      if (alpha[y * width + x] > 16) {
        bottom[x] = y
        break
      }
    }
    if (x < minX) minX = x
    if (x > maxX) maxX = x
  }
  if (maxX < 0) return []

  const kernelWidth = 41
  const halfKernel = 20
  const smoothed = new Float32Array(width)
  const prefix = new Float32Array(width + 1)
  for (let x = 0; x < width; x += 1) {
    prefix[x + 1] = prefix[x] + bottom[x]
  }
  for (let x = 0; x < width; x += 1) {
    const start = Math.max(0, x - halfKernel)
    const end = Math.min(width - 1, x + halfKernel)
    smoothed[x] = (prefix[end + 1] - prefix[start]) / kernelWidth
  }
  const peaks = findPeaks(smoothed, minSeparation, 10)
  const positions: number[] = []
  for (let index = 0; index < peaks.length && positions.length < wanted; index += 1) {
    positions.push(peaks[index].x)
  }
  let guard = 0
  while (positions.length < wanted && guard++ < 50) {
    let best = -1
    let bestDistance = -1
    for (let sample = 0; sample < 40; sample += 1) {
      const centerX = Math.round(
        minX + 30 + ((maxX - minX - 60) * sample) / 39,
      )
      if (centerX < 0 || centerX >= width || top[centerX] < 0) continue
      let minimumDistance = 1e9
      for (const position of positions) {
        minimumDistance = Math.min(
          minimumDistance,
          Math.abs(centerX - position),
        )
      }
      if (positions.length === 0) minimumDistance = 1e9 - sample
      if (minimumDistance > bestDistance) {
        bestDistance = minimumDistance
        best = centerX
      }
    }
    if (best < 0) break
    positions.push(best)
  }
  positions.sort((left, right) => left - right)
  const strands: UpstreamHairStrand[] = []
  for (const x of positions) {
    if (top[x] < 0) continue
    strands.push({ x, tipY: bottom[x], rootY: top[x] })
  }
  return strands
}

function makePart(
  name: string,
  layer: UpstreamPsdLayer & { imageData: UpstreamPixelImage },
  fullAlpha: Uint8Array,
  mask: Uint8Array | null,
  canvasWidth: number,
  canvasHeight: number,
  slot: Slot,
  z: number,
  side: UpstreamLayerSide,
  strands: UpstreamHairStrand[] | null,
): UpstreamRigLayer | null {
  let effectiveAlpha = fullAlpha
  if (mask) {
    effectiveAlpha = new Uint8Array(canvasWidth * canvasHeight)
    for (let index = 0; index < canvasWidth * canvasHeight; index += 1) {
      effectiveAlpha[index] = mask[index] ? fullAlpha[index] : 0
    }
  }
  const bounds = bboxOf(effectiveAlpha, canvasWidth, canvasHeight, 8)
  if (!bounds) return null
  const padding = 2
  const x0 = Math.max(0, bounds.x0 - padding)
  const y0 = Math.max(0, bounds.y0 - padding)
  const x1 = Math.min(canvasWidth, bounds.x1 + 1 + padding)
  const y1 = Math.min(canvasHeight, bounds.y1 + 1 + padding)
  const width = x1 - x0
  const height = y1 - y0
  const data = new Uint8ClampedArray(width * height * 4)
  const imageData = layer.imageData
  const layerWidth = imageData.width
  const layerHeight = imageData.height
  const layerX = layer.left! | 0
  const layerY = layer.top! | 0
  const layerData = imageData.data
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      const destinationIndex = ((y - y0) * width + (x - x0)) * 4
      const sourceY = y - layerY
      const sourceX = x - layerX
      if (
        sourceY >= 0 &&
        sourceY < layerHeight &&
        sourceX >= 0 &&
        sourceX < layerWidth
      ) {
        const sourceIndex = (sourceY * layerWidth + sourceX) * 4
        data[destinationIndex] = layerData[sourceIndex]
        data[destinationIndex + 1] = layerData[sourceIndex + 1]
        data[destinationIndex + 2] = layerData[sourceIndex + 2]
      }
      data[destinationIndex + 3] = effectiveAlpha[y * canvasWidth + x]
    }
  }
  return {
    name,
    x: x0,
    y: y0,
    w: width,
    h: height,
    z,
    depth: slot.depth,
    group: slot.group,
    phys: slot.phys || null,
    fade: slot.fade || null,
    side: side || null,
    strands: strands || null,
    img: { width, height, data },
  }
}

export function buildRig(
  psd: UpstreamPsd,
  options: UpstreamRiggerOptions = {},
): UpstreamRig {
  // Upstream uses `opts = opts || {}`; keep accepting an explicit null at runtime.
  options = options || {}
  const canvasWidth = psd.width
  const canvasHeight = psd.height
  const warnings: string[] = []
  const children = (psd.children || []).filter(
    (child): child is UpstreamPsdLayer & { imageData: UpstreamPixelImage } =>
      Boolean(child.imageData),
  )
  if (!children.length) {
    throw new Error(
      'レイヤーが見つかりません（グループは未対応・フラット構成にしてください）',
    )
  }

  const entries: Entry[] = []
  for (const child of children) {
    const name = normName(child.name!)
    const alpha = cleanAlpha(
      fullAlphaOf(child, canvasWidth, canvasHeight),
      canvasWidth,
      canvasHeight,
      40,
    )
    entries.push({ name, layer: child, alpha })
  }
  const byName: Record<string, Entry> = {}
  for (const entry of entries) byName[entry.name] = entry

  const faceEntry = byName.face
  let face: UpstreamRig['anchors']['face']
  if (faceEntry) {
    const bounds = bboxOf(faceEntry.alpha, canvasWidth, canvasHeight, 8)!
    const center = centroidOf(faceEntry.alpha, canvasWidth, canvasHeight)!
    face = {
      cx: center.cx,
      cy: center.cy,
      x0: bounds.x0,
      x1: bounds.x1,
      y0: bounds.y0,
      y1: bounds.y1,
    }
  } else {
    warnings.push('face レイヤーがありません — キャンバス中央を顔とみなします')
    face = {
      cx: canvasWidth / 2,
      cy: canvasHeight * 0.3,
      x0: canvasWidth * 0.35,
      x1: canvasWidth * 0.65,
      y0: canvasHeight * 0.1,
      y1: canvasHeight * 0.5,
    }
  }

  const parts: UpstreamRigLayer[] = []
  let z = 0
  const sided: Record<string, Uint8Array> = {}
  for (const entry of entries) {
    const base = baseName(entry.name)
    let slot = SLOTS[base]
    if (!slot) {
      const center = centroidOf(entry.alpha, canvasWidth, canvasHeight)
      slot = {
        depth: 1,
        group: center && center.cy < face.y1 ? 'head' : 'body',
      }
      warnings.push(
        `未知のレイヤー名 "${entry.name}" — ${slot.group} として扱います`,
      )
    }
    if (slot.split) {
      const masks = splitSides(
        entry.alpha,
        canvasWidth,
        canvasHeight,
        face.cx,
      )
      let got = false
      for (const sideKey of ['l', 'r'] as const) {
        const mask = masks[sideKey]
        if (!mask) continue
        const record = makePart(
          `${entry.name}_${sideKey}`,
          entry.layer,
          entry.alpha,
          mask,
          canvasWidth,
          canvasHeight,
          slot,
          z,
          sideKey.toUpperCase() as 'L' | 'R',
          null,
        )
        if (!record) continue
        parts.push(record)
        z += 1
        const maskedAlpha = new Uint8Array(canvasWidth * canvasHeight)
        for (let index = 0; index < canvasWidth * canvasHeight; index += 1) {
          maskedAlpha[index] = mask[index] ? entry.alpha[index] : 0
        }
        sided[`${entry.name}|${sideKey}`] = maskedAlpha
        got = true
      }
      if (!got) {
        warnings.push(`"${entry.name}" の左右分離に失敗（空レイヤー？）`)
      }
    } else if (slot.phys === 'hair') {
      const isPart = /_\d+$/.test(entry.name)
      const bounds = bboxOf(entry.alpha, canvasWidth, canvasHeight, 16)
      const pixelWidth = bounds ? bounds.x1 - bounds.x0 : 0
      const wanted = isPart
        ? Math.max(2, Math.min(6, Math.round(pixelWidth / 110)))
        : 6
      const minSeparation = Math.max(
        30,
        Math.round(pixelWidth / (wanted * 1.6)),
      )
      const strands = detectStrands(
        entry.alpha,
        canvasWidth,
        canvasHeight,
        minSeparation,
        wanted,
      )
      const record = makePart(
        entry.name,
        entry.layer,
        entry.alpha,
        null,
        canvasWidth,
        canvasHeight,
        slot,
        z,
        null,
        strands,
      )
      if (record) {
        parts.push(record)
        z += 1
      }
    } else {
      const record = makePart(
        entry.name,
        entry.layer,
        entry.alpha,
        null,
        canvasWidth,
        canvasHeight,
        slot,
        z,
        null,
        null,
      )
      if (record) {
        parts.push(record)
        z += 1
      }
    }
  }

  let eyeL: UpstreamEyeAnchor | undefined
  let eyeR: UpstreamEyeAnchor | undefined
  for (const sideKey of ['l', 'r'] as const) {
    const eyeWhite = sided[`eyewhite|${sideKey}`]
    const iris = sided[`irides|${sideKey}`]
    const eyeClose = sided[`eye_close|${sideKey}`]
    if (!eyeWhite) continue
    const bounds = bboxOf(eyeWhite, canvasWidth, canvasHeight, 8)!
    const irisCenter = centroidOf(
      iris || eyeWhite,
      canvasWidth,
      canvasHeight,
    )!
    const closeCenter = eyeClose
      ? centroidOf(eyeClose, canvasWidth, canvasHeight)
      : null
    const anchor: UpstreamEyeAnchor = {
      x0: bounds.x0,
      x1: bounds.x1,
      y0: bounds.y0,
      y1: bounds.y1,
      icx: irisCenter.cx,
      icy: irisCenter.cy,
      closeY: closeCenter
        ? closeCenter.cy
        : bounds.y0 + (bounds.y1 - bounds.y0) * 0.62,
    }
    if (sideKey === 'l') eyeL = anchor
    else eyeR = anchor
  }
  if (!eyeL || !eyeR) {
    warnings.push('目のアンカーが不完全です（eyewhite/irides を確認）')
  }

  const mouthSource = byName.mouth_open || byName.mouth_close
  let mouth: UpstreamRig['anchors']['mouth']
  if (mouthSource) {
    const bounds = bboxOf(mouthSource.alpha, canvasWidth, canvasHeight, 8)!
    const center = centroidOf(
      mouthSource.alpha,
      canvasWidth,
      canvasHeight,
    )!
    mouth = {
      x0: bounds.x0,
      x1: bounds.x1,
      y0: bounds.y0,
      y1: bounds.y1,
      cx: center.cx,
      cy: center.cy,
    }
  } else {
    warnings.push('mouth_open / mouth_close がありません')
    mouth = {
      x0: face.cx - 20,
      x1: face.cx + 20,
      y0: face.cy + 40,
      y1: face.cy + 60,
      cx: face.cx,
      cy: face.cy + 50,
    }
  }

  const neckEntry = byName.neck
  let neckPivot: { cx: number; cy: number }
  let neckTop: number
  let neckBottom: number
  if (neckEntry) {
    const bounds = bboxOf(neckEntry.alpha, canvasWidth, canvasHeight, 8)!
    const center = centroidOf(neckEntry.alpha, canvasWidth, canvasHeight)!
    neckPivot = {
      cx: center.cx,
      cy: bounds.y0 + (bounds.y1 - bounds.y0) * 0.85,
    }
    neckTop = bounds.y0
    neckBottom = bounds.y1
  } else {
    neckPivot = { cx: face.cx, cy: face.y1 + 20 }
    neckTop = face.y1
    neckBottom = face.y1 + 60
  }
  const anchors: UpstreamRig['anchors'] = {
    face,
    ...(eyeL ? { eyeL } : {}),
    ...(eyeR ? { eyeR } : {}),
    mouth,
    neckPivot,
    neckTop,
    neckBottom,
    bodyPivot: { cx: neckPivot.cx, cy: canvasHeight },
    faceScale: (face.x1 - face.x0) / 333,
    hairRootY: face.y0 + 60,
  }

  const synth = { eye: false, mouth: false }
  const generic = options.generic
  if (generic) synthesizeMissingParts(parts, anchors, byName, generic, synth, warnings)
  for (let index = 0; index < parts.length; index += 1) parts[index].z = index

  return {
    canvas: { w: canvasWidth, h: canvasHeight },
    layers: parts,
    anchors,
    warnings,
    synth,
  }
}

function synthesizeMissingParts(
  parts: UpstreamRigLayer[],
  anchors: UpstreamRig['anchors'],
  byName: Record<string, Entry>,
  generic: UpstreamGenericParts,
  synth: { eye: boolean; mouth: boolean },
  warnings: string[],
): void {
  const findPart = (prefix: string) =>
    parts.filter((part) => part.name.indexOf(prefix) === 0)
  if (
    generic.eyeL &&
    generic.eyeR &&
    !findPart('eye_close').length &&
    anchors.eyeL &&
    anchors.eyeR
  ) {
    const slot = SLOTS.eye_close
    const makeEye = (
      anchor: UpstreamEyeAnchor,
      genericImage: UpstreamRgbaImage,
      side: 'L' | 'R',
    ) => {
      const lash =
        findPart(`eyelash_${side.toLowerCase()}`)[0] ||
        findPart(`eyebrow_${side.toLowerCase()}`)[0]
      const tint = lash
        ? meanColorOfImage(
            lash.img || { data: new Uint8ClampedArray(0) },
            false,
          )
        : null
      return synthPart(
        `eye_close_${side.toLowerCase()}`,
        genericImage,
        (anchor.x1 - anchor.x0) * 1.1,
        (anchor.x0 + anchor.x1) / 2,
        anchor.closeY,
        0.55,
        tint,
        slot,
        side,
      )
    }
    const left = makeEye(anchors.eyeL, generic.eyeL, 'L')
    const right = makeEye(anchors.eyeR, generic.eyeR, 'R')
    let index = lastIndexWhere(parts, (part) =>
      part.name.startsWith('eyelash'),
    )
    if (index < 0) {
      index = lastIndexWhere(parts, (part) => part.name.startsWith('irides'))
    }
    if (index < 0) {
      index = lastIndexWhere(parts, (part) => part.name === 'face')
    }
    parts.splice(index + 1, 0, left, right)
    synth.eye = true
    warnings.push(
      'eye_close が無いため汎用閉じ目を自動配置しました（「目」の差分バーで調整可）',
    )
  }
  if (
    generic.mouth &&
    !findPart('mouth_close').length &&
    byName.mouth_open
  ) {
    const anchor = anchors.mouth
    const mouthOpen = parts.find((part) => part.name === 'mouth_open')
    const tint = mouthOpen ? meanColorOfImage(mouthOpen.img, true) : null
    const close = synthPart(
      'mouth_close',
      generic.mouth,
      (anchor.x1 - anchor.x0) * 1.1,
      anchor.cx,
      anchor.y0 + 0.3 * (anchor.y1 - anchor.y0),
      0.5,
      tint,
      SLOTS.mouth_close,
      null,
    )
    let index = lastIndexWhere(parts, (part) => part.name === 'mouth_open')
    if (index < 0) {
      index = lastIndexWhere(parts, (part) => part.name === 'face')
    }
    parts.splice(index + 1, 0, close)
    synth.mouth = true
    warnings.push(
      'mouth_close が無いため汎用閉じ口を自動配置しました（「口」のバーで調整可）',
    )
  }
}

export function cleanPsdLayers(psd: UpstreamPsd): UpstreamCleanStats {
  const stats: UpstreamCleanStats = { noisy: 0, layers: 0 }
  const children = (psd.children || []).filter(
    (child): child is UpstreamPsdLayer & { imageData: UpstreamPixelImage } =>
      Boolean(child.imageData),
  )
  for (const child of children) {
    const imageData = child.imageData
    const width = imageData.width
    const height = imageData.height
    const data = imageData.data
    stats.layers += 1
    const alpha = new Uint8Array(width * height)
    let before = 0
    let after = 0
    for (let index = 0; index < width * height; index += 1) {
      alpha[index] = data[index * 4 + 3]
      if (alpha[index]) before += 1
    }
    cleanAlpha(alpha, width, height, 40)
    for (let index = 0; index < width * height; index += 1) {
      if (alpha[index]) after += 1
      data[index * 4 + 3] = alpha[index]
    }
    if (after < before) stats.noisy += 1
    const bounds = bboxOf(alpha, width, height, 0)
    if (!bounds) continue
    const padding = 4
    const x0 = Math.max(0, bounds.x0 - padding)
    const y0 = Math.max(0, bounds.y0 - padding)
    const x1 = Math.min(width - 1, bounds.x1 + padding)
    const y1 = Math.min(height - 1, bounds.y1 + padding)
    const trimmedWidth = x1 - x0 + 1
    const trimmedHeight = y1 - y0 + 1
    if (trimmedWidth >= width && trimmedHeight >= height) continue
    const output = new Uint8ClampedArray(trimmedWidth * trimmedHeight * 4)
    for (let y = 0; y < trimmedHeight; y += 1) {
      const sourceOffset = ((y + y0) * width + x0) * 4
      output.set(
        data.subarray(sourceOffset, sourceOffset + trimmedWidth * 4),
        y * trimmedWidth * 4,
      )
    }
    child.imageData = {
      width: trimmedWidth,
      height: trimmedHeight,
      data: output,
    }
    child.left = (child.left! | 0) + x0
    child.top = (child.top! | 0) + y0
    child.right = child.left + trimmedWidth
    child.bottom = child.top + trimmedHeight
    if (child.canvas) child.canvas = undefined
  }
  return stats
}

export const rigger: UpstreamRiggerApi = {
  buildRig,
  normName,
  baseName,
  cleanPsdLayers,
  flattenPsdToImg,
  splitImgLR,
  _internals: {
    findPeaks,
    detectStrands,
    labelComponents,
    cleanAlpha,
  },
}

export default rigger
