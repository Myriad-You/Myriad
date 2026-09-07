import type { ChestWeightField } from './chestPhysics'
import type {
  Anime25DSecondaryDeformationBinding,
  Anime25DSecondaryDeformationFrame,
} from './secondaryDeformation'
import type { Anime25DPlaybackAnchors, Anime25DPlaybackLayer } from './types'
import { isAnime25DRigidAttachment } from '../rig/anime25dLayerSemantics'
import { sampleChestWeight } from './chestPhysics'
import { deformAnime25DSecondaryPoint } from './secondaryDeformation'

interface AttachmentHost {
  source: Anime25DPlaybackLayer
  secondaryDeformation: Anime25DSecondaryDeformationBinding
}

export interface Anime25DAttachmentPixels {
  width: number
  height: number
  pixels: Uint8ClampedArray
}

type ReadAttachmentPixels = (
  source: Anime25DPlaybackLayer,
) => Anime25DAttachmentPixels | null
interface AttachmentSample {
  x: number
  y: number
  weight: number
}

/** Measured once during binding; transparent PSD padding is not an anchor. */
function attachmentFootprint(
  source: Anime25DPlaybackLayer,
  image: Anime25DAttachmentPixels | null,
): AttachmentSample[] {
  if (
    !image ||
    image.width <= 0 ||
    image.height <= 0 ||
    image.pixels.length !== image.width * image.height * 4
  ) {
    return []
  }
  let top = image.height
  let bottom = -1
  let opaquePixels = 0
  for (let y = 0; y < image.height; y++) {
    for (let x = 0; x < image.width; x++) {
      if (image.pixels[(y * image.width + x) * 4 + 3] < 16) continue
      top = Math.min(top, y)
      bottom = Math.max(bottom, y)
      opaquePixels++
    }
  }
  if (bottom < top) return []
  // Hanging ear art follows its visible root. Other coarse upstream regions
  // get an alpha-weighted centre, not an invented anatomical/material label.
  const rootBottom =
    source.role === 'earwear'
      ? Math.min(bottom, top + (bottom - top + 1) * 0.12)
      : bottom
  const stride = Math.max(1, Math.ceil(opaquePixels / 4096))
  const samples: AttachmentSample[] = []
  let index = 0
  for (let y = top; y <= rootBottom; y++) {
    for (let x = 0; x < image.width; x++) {
      const alpha = image.pixels[(y * image.width + x) * 4 + 3]
      if (alpha < 16 || index++ % stride !== 0) continue
      samples.push({
        x: source.x + ((x + 0.5) / image.width) * source.w,
        y: source.y + ((y + 0.5) / image.height) * source.h,
        weight: alpha / 255,
      })
    }
  }
  return samples
}

function attachmentCoverage(
  samples: readonly AttachmentSample[],
  source: Anime25DPlaybackLayer,
  image: Anime25DAttachmentPixels,
): number {
  let weight = 0
  let supported = 0
  for (const sample of samples) {
    weight += sample.weight
    const x = Math.floor(((sample.x - source.x) / source.w) * image.width)
    const y = Math.floor(((sample.y - source.y) / source.h) * image.height)
    if (x < 0 || y < 0 || x >= image.width || y >= image.height) continue
    supported +=
      (sample.weight * image.pixels[(y * image.width + x) * 4 + 3]) / 255
  }
  return weight > 0 ? supported / weight : 0
}

export interface Anime25DLayerAttachment {
  hostName: string
  x: number
  y: number
  binding: Anime25DSecondaryDeformationBinding
  origin: { x: number; y: number }
  tangent: { x: number; y: number }
}

/**
 * Independent drawings ride a surface frame, not its per-vertex strain.
 * Neckwear is an upstream region, not a material: it includes necklaces,
 * scarves and ties. With no finer authoring, preserve the complete drawing.
 * No spring, cloth label, high-collar flag or semantic bone is invented here.
 */
export function bindAnime25DLayerAttachment(
  source: Anime25DPlaybackLayer,
  hosts: readonly AttachmentHost[],
  anchors: Anime25DPlaybackAnchors,
  chestWeights: ChestWeightField | null,
  canvasWidth: number,
  readPixels?: ReadAttachmentPixels,
): Anime25DLayerAttachment | null {
  if (!isAnime25DRigidAttachment(source)) return null
  const samples = attachmentFootprint(source, readPixels?.(source) ?? null)
  const totalWeight = samples.reduce((sum, point) => sum + point.weight, 0)
  const x =
    totalWeight > 0
      ? samples.reduce((sum, point) => sum + point.x * point.weight, 0) /
        totalWeight
      : source.x + source.w / 2
  const y =
    totalWeight > 0
      ? samples.reduce((sum, point) => sum + point.y * point.weight, 0) /
        totalWeight
      : source.y + source.h * (source.role === 'earwear' ? 0.12 : 0.5)
  const roles =
    source.group === 'head'
      ? source.role === 'headwear'
        ? ['front-hair', 'face']
        : source.role === 'earwear'
          ? ['ears', 'face']
          : ['face']
      : source.role === 'neckwear' && y < anchors.neckBottom
        ? ['neck', 'topwear', 'bottomwear']
        : ['topwear', 'bottomwear', 'neck']
  let host: AttachmentHost | undefined
  let fallbackHost: AttachmentHost | undefined
  for (const role of roles) {
    // Match an explicit side where available, then use the nearest drawing of
    // that host class. Numbered fragments remain independently attached.
    let distance = Number.POSITIVE_INFINITY
    let coverage = -1
    let fallbackDistance = Number.POSITIVE_INFINITY
    let roleFallback: AttachmentHost | undefined
    for (const candidate of hosts) {
      if (candidate.source === source || candidate.source.role !== role)
        continue
      if (
        source.side &&
        candidate.source.side &&
        candidate.source.side !== source.side
      ) {
        continue
      }
      const bounds = candidate.source
      const dx = Math.max(bounds.x - x, 0, x - bounds.x - bounds.w)
      const dy = Math.max(bounds.y - y, 0, y - bounds.y - bounds.h)
      const nextDistance = dx * dx + dy * dy
      if (nextDistance < fallbackDistance) {
        fallbackDistance = nextDistance
        roleFallback = candidate
      }
      const pixels = samples.length ? readPixels?.(candidate.source) : null
      const nextCoverage = pixels
        ? attachmentCoverage(samples, candidate.source, pixels)
        : -1
      // A bounding rectangle can enclose nothing but transparency here. Do not
      // bind to that fragment when a sibling actually contains the attachment.
      if (nextCoverage === 0) continue
      if (
        nextCoverage > coverage ||
        (nextCoverage === coverage && nextDistance < distance)
      ) {
        distance = nextDistance
        coverage = nextCoverage
        host = candidate
      }
    }
    fallbackHost ??= roleFallback
    if (host) break
  }
  // Detached/hanging art may have no alpha intersection at all. Keep its
  // semantic surface fallback instead of losing shell follow in that case.
  host ??= fallbackHost
  if (!host) return null
  const binding = { ...host.secondaryDeformation }
  // The host may normally put its global motion in a shader (e.g. face with
  // shell projection disabled). These CPU samples must include that motion.
  binding.shaderGlobalTransform = false
  // Host mesh weights are indexed by mesh vertices. Sample this attachment's
  // two points explicitly rather than accidentally reading host vertex zero.
  binding.chestWeights =
    binding.topwear && chestWeights
      ? Float32Array.from([
          sampleChestWeight(chestWeights, x / canvasWidth, y / canvasWidth),
          sampleChestWeight(
            chestWeights,
            (x + 1) / canvasWidth,
            y / canvasWidth,
          ),
        ])
      : null
  binding.hairlinePinWeights = null
  binding.frontHairParallaxScale = null
  return {
    hostName: host.source.name,
    x,
    y,
    binding,
    origin: { x, y },
    tangent: { x: x + 1, y },
  }
}

/**
 * Two host evaluations per layer/frame; a rigid matrix does all GPU work.
 * The tangent carries rotation, but never scale/shear. Gems, glasses and
 * ornaments therefore cannot be squeezed by the torso or skull surface.
 */
export function writeAnime25DAttachmentTransform(
  attachment: Anime25DLayerAttachment,
  frame: Readonly<Anime25DSecondaryDeformationFrame>,
  output: Float32Array,
): void {
  const { x, y, origin, tangent, binding } = attachment
  origin.x = x
  origin.y = y
  tangent.x = x + 1
  tangent.y = y
  deformAnime25DSecondaryPoint(origin, x, y, 0, binding, frame)
  deformAnime25DSecondaryPoint(tangent, x + 1, y, 1, binding, frame)
  const dx = tangent.x - origin.x
  const dy = tangent.y - origin.y
  const length = Math.hypot(dx, dy)
  const cosine = length > 1e-6 ? dx / length : 1
  const sine = length > 1e-6 ? dy / length : 0
  output[0] = cosine
  output[1] = sine
  output[2] = 0
  output[3] = -sine
  output[4] = cosine
  output[5] = 0
  output[6] = origin.x - cosine * x + sine * y
  output[7] = origin.y - sine * x - cosine * y
  output[8] = 1
}
