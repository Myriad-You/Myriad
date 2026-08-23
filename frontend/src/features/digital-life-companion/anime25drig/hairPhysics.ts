const COMPOSITE_TAIL_START = 0.2
const COMPOSITE_TAIL_END = 0.45
const FRONT_HAIR_UPPER_PARALLAX_FLOOR = 0.2
const FRONT_HAIR_UPPER_RELEASE_START = 0.45
const FRONT_HAIR_UPPER_RELEASE_END = 0.75

interface LayerVerticalBounds {
  y: number
  h: number
}

interface FaceVerticalBounds {
  y0: number
  y1: number
}

/**
 * Reduce only the upper excess-depth parallax of a composite front-hair layer.
 * The face-plane head motion remains intact; the retained depth motion then
 * smoothly returns to the authored amount above the long-lock tips.
 */
export function frontHairUpperParallaxScale(
  vertexY: number,
  layer: LayerVerticalBounds,
  face: FaceVerticalBounds,
): number {
  const layerProgress = clamp((vertexY - layer.y) / Math.max(1, layer.h), 0, 1)
  const lowerRelease = smoothstep(
    (layerProgress - FRONT_HAIR_UPPER_RELEASE_START) /
      (FRONT_HAIR_UPPER_RELEASE_END - FRONT_HAIR_UPPER_RELEASE_START),
  )
  const compositeScale =
    FRONT_HAIR_UPPER_PARALLAX_FLOOR +
    (1 - FRONT_HAIR_UPPER_PARALLAX_FLOOR) * lowerRelease
  const compositeMix = compositeHairMix(layer, face)
  return 1 - compositeMix * (1 - compositeScale)
}

function compositeHairMix(
  layer: LayerVerticalBounds,
  face: FaceVerticalBounds,
): number {
  const faceHeight = Math.max(1, face.y1 - face.y0)
  const tailRatio = Math.max(0, layer.y + layer.h - face.y1) / faceHeight
  return smoothstep(
    (tailRatio - COMPOSITE_TAIL_START) /
      (COMPOSITE_TAIL_END - COMPOSITE_TAIL_START),
  )
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
