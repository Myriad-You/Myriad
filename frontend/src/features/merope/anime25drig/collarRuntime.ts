import type { FrontCollarContactModel } from './collarContact'
import type { Anime25DTorsoShellRotation } from './torsoDeformation'
import type { Anime25DPlaybackLayer, Anime25DTorsoShellProfile } from './types'
import { localToAtlasUv } from './atlasUv'
import { anime25DTorsoShellOffsetX } from './torsoDeformation'
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
  x +=
    headFollow *
    pose.faceScale *
    (pose.angleX * (14 + 40 * depthOffset) +
      pose.angleX * (pose.neckPivotY - y) * 0.028)
  y +=
    headFollow *
    pose.faceScale *
    (-pose.angleY * (9 + 30 * depthOffset) -
      pose.angleY * depthOffset * (y - pose.faceCenterY) * 0.05)
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
    clip.deformed[index] += anime25DTorsoShellOffsetX(
      clip.deformed[index],
      pose.torsoProfile,
      pose.torsoShellRotation,
      pose.torsoShellBlend * (1 - headBlend),
    )
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
