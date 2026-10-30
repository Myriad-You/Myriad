import type { FrontCollarContactModel } from './collarContact'
import type { HeadTurn } from './headTurn'
import type { Anime25DTorsoShellRotation } from './torsoDeformation'
import type { Anime25DPlaybackLayer, Anime25DTorsoShellProfile } from './types'
import { localToAtlasUv } from './atlasUv'
import { carryOnFigure } from './figureSkeleton'
import { headTurnNeckMove } from './headTurn'
import { anime25DTorsoShellOffsetX, figureFrontDepth } from './torsoDeformation'
import {
  createIndexedDeformableMesh,
  disposeIndexedDeformableMesh,
} from './webglRuntime'

export const BODY_HEAD_FOLLOW = 0.16
export const HIGH_COLLAR_NECK_FOLLOW_POWER = 3
export const FRONT_COLLAR_HEAD_FOLLOW = 0.42
export const FRONT_COLLAR_FLEX_REGION = 0.78
export const FRONT_COLLAR_INNER_REGION = 0.72

export interface CollarClipMesh {
  rest: Float32Array
  deformed: Float32Array
  atlasUvs: Float32Array
  indices: Uint16Array
  vao: WebGLVertexArrayObject
  vertexBuffer: WebGLBuffer
  uvBuffer: WebGLBuffer
  indexBuffer: WebGLBuffer
  indexCount: number
}

export interface CollarMotionPose {
  neckPivotX: number
  neckPivotY: number
  neckFollowTop: number
  neckFollowSpan: number
  faceCenterY: number
  faceScale: number
  angleX: number
  angleY: number
  headRotationCosine: number
  headRotationSine: number
  bodyBreathOffset: number
  headBreathOffset: number
  torsoProfile: Readonly<Anime25DTorsoShellProfile>
  torsoShellRotation: Readonly<Anime25DTorsoShellRotation>
  torsoShellBlend: number
  torsoNeckOffsetX: number
  /** The neck's foot and the chest's middle, between which a figure's front comes out from the neck's depth (figureFrontDepth). */
  neckBottom: number
  chestRegionCenterY: number
  /** The computed head, which the top of the neck follows at the jaw. */
  headTurn: Readonly<HeadTurn> | null
  /** A keyed head turns on a still body: the collar takes no part of the turn or the nod. */
  keyedHead: boolean
}

/** The collar's pose this frame: the head's turn and roll, breath and the torso's carry. */
export function writeCollarMotion(
  pose: CollarMotionPose,
  driver: Readonly<{ angleX: number; angleY: number }>,
  frame: Readonly<Pick<CollarMotionPose, 'headRotationCosine' | 'headRotationSine' | 'bodyBreathOffset' |
    'headBreathOffset' | 'torsoShellBlend' | 'torsoNeckOffsetX'>>,
): CollarMotionPose {
  pose.angleX = driver.angleX
  pose.angleY = driver.angleY
  pose.headRotationCosine = frame.headRotationCosine
  pose.headRotationSine = frame.headRotationSine
  pose.bodyBreathOffset = frame.bodyBreathOffset
  pose.headBreathOffset = frame.headBreathOffset
  pose.torsoShellBlend = frame.torsoShellBlend
  pose.torsoNeckOffsetX = frame.torsoNeckOffsetX
  return pose
}

export function collarNeckHeadBlend(
  y: number,
  pose: Pick<CollarMotionPose, 'neckFollowTop' | 'neckFollowSpan'>,
): number {
  const progress = clamp(
    (pose.neckFollowTop + pose.neckFollowSpan - y) / pose.neckFollowSpan,
    0,
    1,
  )
  return smoothstep(progress ** HIGH_COLLAR_NECK_FOLLOW_POWER)
}

function transformCollarPoint(
  restX: number,
  restY: number,
  headFollow: number,
  depth: number,
  neckHeadBlend: number,
  attachedToNeck: boolean,
  pose: CollarMotionPose,
  output: Float32Array,
  index: number,
): void {
  let x = restX
  let y = restY
  const rotationX = x - pose.neckPivotX
  const rotationY = y - pose.neckPivotY
  const rotatedX =
    rotationX * pose.headRotationCosine - rotationY * pose.headRotationSine
  const rotatedY =
    rotationX * pose.headRotationSine + rotationY * pose.headRotationCosine
  x += (rotatedX - rotationX) * headFollow
  y += (rotatedY - rotationY) * headFollow
  let depthOffset = depth - 1
  if (attachedToNeck) depthOffset *= 1 - neckHeadBlend
  const turnFollow = pose.keyedHead ? 0 : headFollow
  const carryX =
    turnFollow *
    pose.faceScale *
    (pose.angleX * (14 + 40 * depthOffset) +
      pose.angleX * (pose.neckPivotY - y) * 0.028)
  const carryY =
    turnFollow *
    pose.faceScale *
    (-pose.angleY * (9 + 30 * depthOffset) -
      pose.angleY * depthOffset * (y - pose.faceCenterY) * 0.05)
  x += carryX
  y += carryY
  // The top of the neck goes with the chin, as the neck's own mesh does.
  if (attachedToNeck && neckHeadBlend > 0 && pose.headTurn?.active && !pose.keyedHead) {
    const jaw = headTurnNeckMove(pose.headTurn, restX, restY)
    x += (jaw.x - carryX) * neckHeadBlend
    y += (jaw.y - carryY) * neckHeadBlend
  }
  const breathOffset = attachedToNeck
    ? pose.bodyBreathOffset +
      (pose.headBreathOffset - pose.bodyBreathOffset) * neckHeadBlend
    : pose.bodyBreathOffset
  output[index] = x
  output[index + 1] = y - breathOffset * pose.faceScale
}

export function createCollarClipMesh(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  model: FrontCollarContactModel,
  neck: Anime25DPlaybackLayer,
  collar: Anime25DPlaybackLayer,
): CollarClipMesh {
  const firstLeftIndex = model.contactPairs[0] * 2
  const firstRightIndex = model.contactPairs[1] * 2
  const firstY = model.handles[firstLeftIndex + 1]
  const transitionHeight = Math.max(7, Math.min(collar.h * 0.16, neck.h * 0.1))
  const transitionY = Math.max(neck.y, firstY - transitionHeight)
  const horizontalMargin = collar.w * 0.075
  const seamAllowance = Math.max(1, Math.min(2.5, collar.w * 0.01))
  const restValues = [
    neck.x,
    neck.y,
    neck.x + neck.w,
    neck.y,
    Math.max(neck.x, model.handles[firstLeftIndex] - horizontalMargin),
    transitionY,
    Math.min(
      neck.x + neck.w,
      model.handles[firstRightIndex] + horizontalMargin,
    ),
    transitionY,
  ]
  for (let pair = 0; pair < model.contactPairs.length; pair += 2) {
    const leftIndex = model.contactPairs[pair] * 2
    const rightIndex = model.contactPairs[pair + 1] * 2
    restValues.push(
      Math.max(neck.x, model.handles[leftIndex] - seamAllowance),
      model.handles[leftIndex + 1],
      Math.min(neck.x + neck.w, model.handles[rightIndex] + seamAllowance),
      model.handles[rightIndex + 1],
    )
  }
  const lastContactY = restValues.at(-1) ?? transitionY
  if (model.closurePoint && model.closurePoint.y > lastContactY) {
    restValues.push(
      model.closurePoint.x - seamAllowance,
      model.closurePoint.y,
      model.closurePoint.x + seamAllowance,
      model.closurePoint.y,
    )
  } else if (!model.closurePoint) {
    // The opening goes on below where its contacts and closure are looked
    // for (a deep V): the neck shows on down through it, as wide as at the
    // last contact; the collar's sides in front of it cover the rest.
    const bottom = Math.min(neck.y + neck.h, collar.y + collar.h)
    if (bottom > lastContactY) {
      restValues.push(restValues.at(-4)!, bottom, restValues.at(-2)!, bottom)
    }
  }
  const rest = Float32Array.from(restValues)
  const deformed = rest.slice()
  const uvs = new Float32Array(rest.length)
  for (let index = 0; index < rest.length; index += 2) {
    const [u, v] = localToAtlasUv(
      neck.atlas,
      (rest[index] - neck.x) / Math.max(1, neck.w),
      (rest[index + 1] - neck.y) / Math.max(1, neck.h),
    )
    uvs[index] = u
    uvs[index + 1] = v
  }
  const rowCount = rest.length / 4
  const indices = new Uint16Array((rowCount - 1) * 6)
  for (let row = 0; row < rowCount - 1; row += 1) {
    const topLeft = row * 2
    const topRight = topLeft + 1
    const bottomLeft = topLeft + 2
    const bottomRight = topLeft + 3
    indices.set(
      [topLeft, topRight, bottomLeft, topRight, bottomRight, bottomLeft],
      row * 6,
    )
  }
  const mesh = createIndexedDeformableMesh(gl, program, deformed, uvs, indices)
  return {
    rest,
    deformed,
    atlasUvs: uvs,
    indices,
    vao: mesh.vao,
    vertexBuffer: mesh.positionBuffer,
    uvBuffer: mesh.uvBuffer,
    indexBuffer: mesh.indexBuffer,
    indexCount: indices.length,
  }
}

export function deformCollarClipMesh(
  clip: CollarClipMesh,
  pose: CollarMotionPose,
  neckDepth: number,
): void {
  for (let index = 0; index < clip.rest.length; index += 2) {
    const headBlend = collarNeckHeadBlend(clip.rest[index + 1], pose)
    transformCollarPoint(
      clip.rest[index],
      clip.rest[index + 1],
      BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * headBlend,
      neckDepth,
      headBlend,
      true,
      pose,
      clip.deformed,
      index,
    )
    // The neck the clip draws turns as near the axis as the neck's own mesh
    // does: a figure's neck at its depth, not the whole trunk's, or the
    // opening runs off sideways and the cloth behind shows through it.
    clip.deformed[index] += anime25DTorsoShellOffsetX(
      clip.deformed[index],
      pose.torsoProfile,
      pose.torsoShellRotation,
      pose.torsoShellBlend * (1 - headBlend),
      figureFrontDepth(pose.torsoProfile, clip.rest[index + 1], pose.neckBottom, pose.chestRegionCenterY),
    )
    clip.deformed[index] += pose.torsoNeckOffsetX * headBlend
  }
}

/** Beside the neck, a front collar's share of the neck's motion is gone this far out, as a share of the neck's width there. */
export const FRONT_COLLAR_SIDE_FADE = 0.5

/**
 * How much of the neck's motion each vertex of a front collar (rest x, y
 * pairs) takes: all of it over the neck, fading out beside it. Null without
 * a neck to read. A collar
 * beside the neck at rest joins the front collar so a turning neck goes
 * behind it, but it is cloth on the shoulders: carried along with the neck
 * all the way out, its edge with the rest of the top would tear open.
 */
export function frontCollarNeckWeights(
  rest: Float32Array,
  layers: readonly Anime25DPlaybackLayer[],
  readPixels: (layer: Anime25DPlaybackLayer) => Readonly<{ pixels: Uint8ClampedArray; width: number; height: number }> | null,
): Float32Array | null {
  const neck = layers.find((layer) => layer.role === 'neck')
  const neckImage = neck ? readPixels(neck) : null
  if (!neck || !neckImage) return null
  const { pixels, width, height } = neckImage
  const weights = new Float32Array(rest.length / 2).fill(1)
  if (width < 1 || height < 1 || pixels.length !== width * height * 4) return weights
  const spans: Array<[number, number] | null> = []
  for (let row = 0; row < height; row++) {
    let left = -1
    let right = -1
    for (let x = 0; x < width; x++) {
      if (pixels[(row * width + x) * 4 + 3] < 128) continue
      if (left < 0) left = x
      right = x
    }
    spans.push(left < 0 ? null : [neck.x + (left / width) * neck.w, neck.x + ((right + 1) / width) * neck.w])
  }
  for (let vertex = 0; vertex < weights.length; vertex++) {
    const x = rest[vertex * 2]
    const row = Math.floor(((rest[vertex * 2 + 1] - neck.y) / neck.h) * height)
    const span = spans[Math.max(0, Math.min(height - 1, row))]
    if (!span) continue
    const outside = Math.max(span[0] - x, x - span[1], 0)
    const fade = Math.max(1, (span[1] - span[0]) * FRONT_COLLAR_SIDE_FADE)
    weights[vertex] = smoothstep(1 - outside / fade)
  }
  return weights
}

/**
 * A standing figure's bones carry every layer (figureSkeleton), the neck
 * the clip draws through too: the clip goes where they carry the neck, or
 * the neck stays drawn where it stood while the figure sways away from it.
 * A bust has no figure, and nothing changes.
 */
export function carryCollarClipOnFigure(
  clip: CollarClipMesh,
  binding: Parameters<typeof carryOnFigure>[1] | undefined,
  frame: Parameters<typeof carryOnFigure>[4],
): void {
  if (!frame.figure || !binding) return
  const point = { x: 0, y: 0 }
  for (let index = 0; index < clip.rest.length; index += 2) {
    point.x = clip.deformed[index]
    point.y = clip.deformed[index + 1]
    carryOnFigure(point, binding, clip.rest[index], clip.rest[index + 1], frame)
    clip.deformed[index] = point.x
    clip.deformed[index + 1] = point.y
  }
}

export function uploadCollarClipMesh(
  gl: WebGL2RenderingContext,
  clip: CollarClipMesh,
): void {
  gl.bindBuffer(gl.ARRAY_BUFFER, clip.vertexBuffer)
  gl.bufferSubData(gl.ARRAY_BUFFER, 0, clip.deformed)
}

export function disposeCollarClipMesh(
  gl: WebGL2RenderingContext,
  clip: Readonly<CollarClipMesh>,
): void {
  disposeIndexedDeformableMesh(gl, {
    vao: clip.vao,
    positionBuffer: clip.vertexBuffer,
    uvBuffer: clip.uvBuffer,
    indexBuffer: clip.indexBuffer,
  })
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
