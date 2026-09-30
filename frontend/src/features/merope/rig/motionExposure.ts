import type { EyeSide, RasterLayer } from './anime25dImportTypes'

/**
 * What a fading part uncovers. A decomposed portrait composites back to the
 * original at rest, yet a blink or a spoken vowel fades a part off the skin
 * meant to lie below it; wherever that skin was never painted, the rig opens a
 * hole. Holes are transparent pixels the skin's own paint closes in on, so the
 * silhouette is never counted, and hair behind the face cannot hide a face
 * left unpainted. (A glance needs no check: irises are clipped to the white.)
 */
export type MotionExposureCheck = 'face-under-eyes' | 'face-under-mouth'

export interface MotionExposureFinding {
  check: MotionExposureCheck
  side: EyeSide | null
  /** Hole pixels the motion uncovers, as a share of the moving part's paint. */
  share: number
  /** Where the hole opens, in layer (canvas) pixels. */
  bounds: { x: number; y: number; width: number; height: number }
}

const OPAQUE = 128
// Below this the uncovered hole is a stray pixel or two at an anti-aliased rim.
const REPORT_SHARE = 0.02

interface Mask {
  left: number
  top: number
  width: number
  height: number
  data: Uint8Array
}

export function findMotionExposure(layers: readonly RasterLayer[]): MotionExposureFinding[] {
  const findings: MotionExposureFinding[] = []
  // Only skin belongs under the eyes and the mouth.
  const skin = layers.filter((layer) => layer.role === 'face' || layer.role === 'facedetail')
  const report = (finding: MotionExposureFinding | null) => {
    if (finding && finding.share >= REPORT_SHARE) findings.push(finding)
  }

  for (const side of ['left', 'right'] as const) {
    const eye = layers.filter((layer) =>
      layer.side === side &&
      (layer.role === 'eyewhite' || layer.role === 'irides' || layer.role === 'eyelash'))
    // A blink fades the open eye out entirely.
    report(fadeExposure('face-under-eyes', side, eye, skin))
  }

  const mouth = layers.filter((layer) => layer.role === 'mouth-close')
  report(fadeExposure('face-under-mouth', null, mouth, skin))
  return findings
}

/** Every pixel of a fading part is uncovered once it has faded out. */
function fadeExposure(
  check: MotionExposureCheck,
  side: EyeSide | null,
  parts: readonly RasterLayer[],
  below: readonly RasterLayer[],
): MotionExposureFinding | null {
  if (parts.length === 0) return null
  const moving = unionMask(parts, 0)
  return measure(check, side, moving, enclosedHoles(unionMask(below, 1, moving)))
}

function measure(
  check: MotionExposureCheck,
  side: EyeSide | null,
  moving: Mask,
  holes: Mask,
): MotionExposureFinding | null {
  let area = 0
  for (const value of moving.data) area += value
  if (area === 0) return null
  let count = 0
  let x0 = Infinity
  let y0 = Infinity
  let x1 = -Infinity
  let y1 = -Infinity
  for (let y = 0; y < moving.height; y += 1) {
    for (let x = 0; x < moving.width; x += 1) {
      if (!moving.data[y * moving.width + x]) continue
      // The hole map is padded by one pixel so the outside stays connected.
      if (!holes.data[(y + 1) * holes.width + x + 1]) continue
      count += 1
      x0 = Math.min(x0, x)
      y0 = Math.min(y0, y)
      x1 = Math.max(x1, x)
      y1 = Math.max(y1, y)
    }
  }
  if (count === 0) return null
  return {
    check,
    side,
    share: count / area,
    bounds: { x: moving.left + x0, y: moving.top + y0, width: x1 - x0 + 1, height: y1 - y0 + 1 },
  }
}

/** Opaque union of layers, over `frame` grown by `pad` pixels when given. */
function unionMask(layers: readonly RasterLayer[], pad: number, frame?: Mask): Mask {
  let left: number
  let top: number
  let right: number
  let bottom: number
  if (frame) {
    left = frame.left - pad
    top = frame.top - pad
    right = frame.left + frame.width + pad
    bottom = frame.top + frame.height + pad
  } else {
    left = Math.min(...layers.map((layer) => layer.left))
    top = Math.min(...layers.map((layer) => layer.top))
    right = Math.max(...layers.map((layer) => layer.left + layer.width))
    bottom = Math.max(...layers.map((layer) => layer.top + layer.height))
  }
  const width = right - left
  const height = bottom - top
  const data = new Uint8Array(width * height)
  for (const layer of layers) {
    const x0 = Math.max(left, layer.left)
    const y0 = Math.max(top, layer.top)
    const x1 = Math.min(right, layer.left + layer.width)
    const y1 = Math.min(bottom, layer.top + layer.height)
    for (let y = y0; y < y1; y += 1) {
      for (let x = x0; x < x1; x += 1) {
        if (layer.data[((y - layer.top) * layer.width + x - layer.left) * 4 + 3] >= OPAQUE) {
          data[(y - top) * width + x - left] = 1
        }
      }
    }
  }
  return { left, top, width, height, data }
}

/**
 * Transparent pixels the paint closes in on every side. Paint reaching past
 * the frame is treated as closed, since a part's surroundings continue there;
 * only the frame's padding ring counts as outside.
 */
function enclosedHoles(paint: Mask): Mask {
  const { width, height, data } = paint
  const outside = new Uint8Array(width * height)
  const stack: number[] = []
  for (let x = 0; x < width; x += 1) {
    stack.push(x, (height - 1) * width + x)
  }
  for (let y = 0; y < height; y += 1) {
    stack.push(y * width, y * width + width - 1)
  }
  while (stack.length > 0) {
    const index = stack.pop()!
    if (outside[index] || data[index]) continue
    outside[index] = 1
    const x = index % width
    if (x > 0) stack.push(index - 1)
    if (x < width - 1) stack.push(index + 1)
    if (index >= width) stack.push(index - width)
    if (index < (height - 1) * width) stack.push(index + width)
  }
  const holes = new Uint8Array(width * height)
  for (let index = 0; index < holes.length; index += 1) {
    holes[index] = data[index] || outside[index] ? 0 : 1
  }
  return { ...paint, data: holes }
}
