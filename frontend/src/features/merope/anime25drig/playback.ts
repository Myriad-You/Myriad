import type { Anime25DLayerRole } from '../rig/anime25d'
import type {
  Anime25DEyeAnchor,
  Anime25DFade,
  Anime25DMouthProfile,
  Anime25DPlayback,
  Anime25DPlaybackAnchors,
  Anime25DPlaybackLayer,
} from './types'
import { ANIME25D_LAYER_DEPTH, anime25DLayerFade } from '../rig/anime25d'
import { deriveGeometryChestProfile } from './chestPhysics'
import { deriveAnime25DShellProfile } from './shellProfile'
import { anime25DPlaybackSource } from './types'

export interface Anime25DPlaybackBuildLayer {
  id: string
  role: string
  side: 'left' | 'right' | null
  group: 'head' | 'body'
  bounds: { x: number; y: number; width: number; height: number }
  textureBounds: { x: number; y: number; width: number; height: number }
  strands: Array<{ x: number; rootY: number; tipY: number }>
}

/** Raw `rig.anchors` from Anime2.5DRig `buildRig`. */
export interface Anime25DRiggerAnchors {
  face: {
    cx: number
    cy: number
    x0: number
    x1: number
    y0: number
    y1: number
  }
  eyeL?: Anime25DEyeAnchor
  eyeR?: Anime25DEyeAnchor
  mouth: {
    x0: number
    x1: number
    y0: number
    y1: number
    cx: number
    cy: number
  }
  neckPivot: { cx: number; cy: number }
  neckTop: number
  neckBottom: number
  bodyPivot: { cx: number; cy: number }
  faceScale: number
}

export interface Anime25DPlaybackBuildInput {
  frameWidth: number
  frameHeight: number
  layers: Anime25DPlaybackBuildLayer[]
  anchors: Anime25DPlaybackAnchors
  mouthProfile: Anime25DMouthProfile
}

/** Translate Anime2.5DRig document anchors into the 3:4 content frame. */
export function remapRiggerAnchors(
  anchors: Anime25DRiggerAnchors,
  frame: { x: number; y: number; width: number; height: number },
): Anime25DPlaybackAnchors {
  const shiftX = (value: number) => value - frame.x
  const shiftY = (value: number) => value - frame.y
  const neckPivot = {
    x: shiftX(anchors.neckPivot.cx),
    y: shiftY(anchors.neckPivot.cy),
  }
  return {
    face: {
      x0: shiftX(anchors.face.x0),
      y0: shiftY(anchors.face.y0),
      x1: shiftX(anchors.face.x1),
      y1: shiftY(anchors.face.y1),
      cx: shiftX(anchors.face.cx),
      cy: shiftY(anchors.face.cy),
    },
    neckPivot,
    neckTop: shiftY(anchors.neckTop),
    neckBottom: shiftY(anchors.neckBottom),
    bodyPivot: { x: neckPivot.x, y: frame.height },
    mouth: {
      x0: shiftX(anchors.mouth.x0),
      y0: shiftY(anchors.mouth.y0),
      x1: shiftX(anchors.mouth.x1),
      y1: shiftY(anchors.mouth.y1),
      cx: shiftX(anchors.mouth.cx),
      cy: shiftY(anchors.mouth.cy),
    },
    faceScale: anchors.faceScale,
    eyeL: shiftEyeAnchor(anchors.eyeL, shiftX, shiftY),
    eyeR: shiftEyeAnchor(anchors.eyeR, shiftX, shiftY),
  }
}

function shiftEyeAnchor(
  eye: Anime25DEyeAnchor | undefined,
  shiftX: (value: number) => number,
  shiftY: (value: number) => number,
): Anime25DEyeAnchor | undefined {
  if (!eye) return undefined
  return {
    x0: shiftX(eye.x0),
    y0: shiftY(eye.y0),
    x1: shiftX(eye.x1),
    y1: shiftY(eye.y1),
    icx: shiftX(eye.icx),
    icy: shiftY(eye.icy),
    closeY: shiftY(eye.closeY),
  }
}

export function buildAnime25DPlayback(
  input: Anime25DPlaybackBuildInput,
  copy: { anime25dMissingLayer: string },
): Anime25DPlayback {
  const width = Math.max(1, input.frameWidth)
  const height = Math.max(1, input.frameHeight)
  const layers = input.layers.map((layer, index) =>
    toPlaybackLayer(layer, width, index),
  )
  requiredLayer(layers, 'face', copy)
  const source = {
    ...anime25DPlaybackSource(),
    pixelCanvas: { width, height },
    layers,
    anchors: input.anchors,
  }
  return {
    ...source,
    mouthProfile: input.mouthProfile,
    chestProfile: deriveGeometryChestProfile(source),
    shellProfile: deriveAnime25DShellProfile(source),
  }
}

function toPlaybackLayer(
  layer: Anime25DPlaybackBuildLayer,
  frameWidth: number,
  z: number,
): Anime25DPlaybackLayer {
  const role = layer.role
  return {
    name: playbackName(layer),
    role,
    z,
    depth: playbackDepth(role),
    group: layer.group,
    phys: role === 'front-hair' || role === 'back-hair' ? 'hair' : null,
    fade: playbackFade(role),
    side: layer.side === 'left' ? 'L' : layer.side === 'right' ? 'R' : null,
    x: layer.bounds.x * frameWidth,
    y: layer.bounds.y * frameWidth,
    w: layer.bounds.width * frameWidth,
    h: layer.bounds.height * frameWidth,
    atlas: {
      x: layer.textureBounds.x,
      y: layer.textureBounds.y,
      w: layer.textureBounds.width,
      h: layer.textureBounds.height,
    },
    strands: layer.strands.map((strand) => ({
      x: strand.x * frameWidth,
      rootY: strand.rootY * frameWidth,
      tipY: strand.tipY * frameWidth,
    })),
  }
}

function playbackName(layer: Anime25DPlaybackBuildLayer): string {
  if (/-\d+(?:-|$)/.test(layer.id)) return layer.id
  if (layer.side === 'left') return `${layer.role}-L`
  if (layer.side === 'right') return `${layer.role}-R`
  return layer.id
}

function playbackDepth(role: string): number {
  return Object.hasOwn(ANIME25D_LAYER_DEPTH, role)
    ? ANIME25D_LAYER_DEPTH[role as Anime25DLayerRole]
    : 1
}

function playbackFade(role: string): Anime25DFade | null {
  return anime25DLayerFade(role)
}

function requiredLayer(
  layers: Anime25DPlaybackLayer[],
  role: string,
  copy: { anime25dMissingLayer: string },
): Anime25DPlaybackLayer {
  const layer = layers.find((candidate) => candidate.role === role)
  if (!layer) {
    throw new Error(copy.anime25dMissingLayer.replace('{role}', role))
  }
  return layer
}
