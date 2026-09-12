import type { AttachmentMeshSample } from './attachmentMesh'
import type { ChestWeightField } from './chestPhysics'
import type {
  Anime25DSecondaryDeformationBinding,
  Anime25DSecondaryDeformationFrame,
} from './secondaryDeformation'
import type { Anime25DPlaybackAnchors, Anime25DPlaybackLayer } from './types'
import { isAnime25DRigidAttachment } from '../rig/anime25dLayerSemantics'
import { bindAttachmentMesh, offsetAttachmentMeshSample, sampleAttachmentMesh } from './attachmentMesh'
import { sampleChestWeight } from './chestPhysics'
import { deformAnime25DSecondaryPoint } from './secondaryDeformation'

interface AttachmentHost {
  source: Anime25DPlaybackLayer
  secondaryDeformation: Anime25DSecondaryDeformationBinding
  rest?: Float32Array
  deformed?: Float32Array
  indices?: Uint16Array
  layerTransform?: Float32Array
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
  hostSource?: Anime25DPlaybackLayer
  meshSamples?: [AttachmentMeshSample, AttachmentMeshSample]
  x: number
  y: number
  binding: Anime25DSecondaryDeformationBinding
  origin: { x: number; y: number }
  tangent: { x: number; y: number }
}

export interface Anime25DNeckwearBridge {
  upper: Anime25DLayerAttachment
  lower: Anime25DLayerAttachment
  weights: Float32Array
  upperMatrix: Float32Array
  lowerMatrix: Float32Array
}

export function bindNeckwearBridge(
  source: Anime25DPlaybackLayer,
  hosts: readonly AttachmentHost[],
  anchors: Anime25DPlaybackAnchors,
  chestWeights: ChestWeightField | null,
  canvasWidth: number,
  rest: Float32Array,
  readPixels: ReadAttachmentPixels,
): Anime25DNeckwearBridge | null {
  if (
    source.role !== 'neckwear' ||
    source.y >= anchors.neckBottom ||
    source.y + source.h <= anchors.neckBottom
  ) {
    return null
}
  const samples = attachmentFootprint(source, readPixels(source))
  const upperPoints = samples.filter((p) => p.y < anchors.neckBottom)
  const lowerPoints = samples.filter((p) => p.y >= anchors.neckBottom)
  if (upperPoints.length < 8 || lowerPoints.length < 8) return null
  const bind = (points: AttachmentSample[], role: string) => {
    const candidates = hosts.filter((h) => h.source.role === role)
    if (candidates.length !== 1) return null
    const pixels = readPixels(candidates[0].source)
    if (!pixels) return null
    // A knot/ribbon may project beyond the neck. Bind its supported root,
    // rather than requiring the entire ornament to lie on skin.
    const contact = points.filter(
      (p) => attachmentCoverage([p], candidates[0].source, pixels) >= 0.8,
    )
    if (contact.length < 8 || contact.length < points.length * 0.25) return null
    const mass = contact.reduce((s, p) => s + p.weight, 0)
    const x = contact.reduce((s, p) => s + p.x * p.weight, 0) / mass
    const y = contact.reduce((s, p) => s + p.y * p.weight, 0) / mass
    return bindAnime25DLayerAttachment(
      { ...source, x: x - 0.5, y: y - 0.5, w: 1, h: 1 },
      candidates,
      anchors,
      chestWeights,
      canvasWidth,
    )
  }
  const upper = bind(upperPoints, 'neck')
    const lower = bind(lowerPoints, 'topwear')
  if (!upper || !lower) return null
  const span = Math.max(
    1,
    Math.min(source.h * 0.35, (anchors.neckBottom - anchors.neckTop) * 0.5),
  )
  const weights = Float32Array.from({ length: rest.length / 2 }, (_, i) => {
    const t = Math.max(
      0,
      Math.min(1, (rest[i * 2 + 1] - (anchors.neckBottom - span / 2)) / span),
    )
    return t * t * (3 - 2 * t)
  })
  return {
    upper,
    lower,
    weights,
    upperMatrix: new Float32Array(9),
    lowerMatrix: new Float32Array(9),
  }
}

export function deformNeckwearBridge(
  bridge: Anime25DNeckwearBridge,
  frame: Readonly<Anime25DSecondaryDeformationFrame>,
  rest: Float32Array,
  output: Float32Array,
): void {
  writeAnime25DAttachmentTransform(bridge.upper, frame, bridge.upperMatrix)
  writeAnime25DAttachmentTransform(bridge.lower, frame, bridge.lowerMatrix)
  const a = bridge.upperMatrix
    const b = bridge.lowerMatrix
  for (let i = 0; i < bridge.weights.length; i++) {
    const x = rest[i * 2]
      const y = rest[i * 2 + 1]
      const w = bridge.weights[i]
    output[i * 2] =
      (a[0] * x + a[3] * y + a[6]) * (1 - w) + (b[0] * x + b[3] * y + b[6]) * w
    output[i * 2 + 1] =
      (a[1] * x + a[4] * y + a[7]) * (1 - w) + (b[1] * x + b[4] * y + b[7]) * w
  }
}

/** Neckwear is an upstream region, not a material */
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
  let x =
    totalWeight > 0
      ? samples.reduce((sum, point) => sum + point.x * point.weight, 0) /
        totalWeight
      : source.x + source.w / 2
  let y =
    totalWeight > 0
      ? samples.reduce((sum, point) => sum + point.y * point.weight, 0) /
        totalWeight
      : source.y + source.h * (source.role === 'earwear' ? 0.12 : 0.5)
  const roles =
    source.group === 'head'
      ? source.role === 'headwear'
        ? ['front-hair', 'back-hair', 'face']
        : source.role === 'earwear'
          ? ['ears', 'face']
          : ['face']
      : source.role === 'neckwear' && y < anchors.neckBottom
        ? ['neck', 'topwear', 'bottomwear']
        : ['topwear', 'bottomwear', 'neck']
  let host: AttachmentHost | undefined
  let fallbackHost: AttachmentHost | undefined
  let bestCoverage = -1
  for (const role of roles) {
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
      // Do not bind to that fragment when a sibling actually contains the attachment.
      if (nextCoverage === 0) continue
      if (
        nextCoverage > coverage ||
        (nextCoverage === coverage && nextDistance < distance)
      ) {
        distance = nextDistance
        coverage = nextCoverage
        if (nextCoverage > bestCoverage) {
          host = candidate
          bestCoverage = nextCoverage
        }
      }
    }
    fallbackHost ??= roleFallback
    // Compare evidence across semantic roles, not just fragments of the first role.
    if (host && !readPixels) break
  }
  // Keep its semantic surface fallback instead of losing shell follow in that case.
  host ??= fallbackHost
  if (!host) return null
  // Anchor the supported root, not the centre of a pendant protruding beyond
  // its surface. This also keeps mesh sampling inside the actual contact.
  const hostPixels = readPixels?.(host.source)
  if (hostPixels && samples.length) {
    const supported = samples.filter(
      (p) => attachmentCoverage([p], host!.source, hostPixels) > 0.5,
    )
    const mass = supported.reduce((sum, p) => sum + p.weight, 0)
    if (mass > 0) {
      x = supported.reduce((sum, p) => sum + p.x * p.weight, 0) / mass
      y = supported.reduce((sum, p) => sum + p.y * p.weight, 0) / mass
    }
  }
  const binding = { ...host.secondaryDeformation }
  binding.shaderGlobalTransform = false
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
  const mesh =
    host.rest &&
    host.deformed &&
    host.indices &&
    (!host.secondaryDeformation.shaderGlobalTransform || host.layerTransform)
      ? {
          rest: host.rest,
          deformed: host.deformed,
          indices: host.indices,
          transform: host.layerTransform,
        }
      : null
  const originSample = mesh ? bindAttachmentMesh(mesh, x, y) : null
  const tangentSample = originSample ? offsetAttachmentMeshSample(originSample, 1, 0) : null
  return {
    hostName: host.source.name,
    hostSource: host.source,
    meshSamples:
      originSample && tangentSample ? [originSample, tangentSample] : undefined,
    x,
    y,
    binding,
    origin: { x, y },
    tangent: { x: x + 1, y },
  }
}

/** The tangent carries rotation, but never scale/shear. */
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
  if (attachment.meshSamples) {
    sampleAttachmentMesh(attachment.meshSamples[0], origin)
    sampleAttachmentMesh(attachment.meshSamples[1], tangent)
  } else {
    deformAnime25DSecondaryPoint(origin, x, y, 0, binding, frame)
    deformAnime25DSecondaryPoint(tangent, x + 1, y, 1, binding, frame)
  }
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
