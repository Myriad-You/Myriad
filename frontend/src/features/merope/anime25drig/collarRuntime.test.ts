import type { CollarClipMesh, CollarMotionPose } from './collarRuntime'
import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { buildFrontCollarContactModel } from './collarContact'
import {
  collarNeckHeadBlend,
  createCollarClipMesh,
  deformCollarClipMesh,
  uploadCollarClipMesh,
} from './collarRuntime'
import { anime25DTorsoShellOffsetX } from './torsoDeformation'

test('keeps collar CPU deformation separate from its GPU upload', () => {
  const rest = new Float32Array([80, 90, 120, 90, 84, 128, 116, 128])
  const clip: CollarClipMesh = {
    rest,
    deformed: rest.slice(),
    vao: {} as WebGLVertexArrayObject,
    vertexBuffer: {} as WebGLBuffer,
    uvBuffer: {} as WebGLBuffer,
    indexBuffer: {} as WebGLBuffer,
    indexCount: 6,
  }
  const pose: CollarMotionPose = {
    neckPivotX: 100,
    neckPivotY: 150,
    neckFollowTop: 82,
    neckFollowSpan: 70,
    faceCenterY: 92,
    faceScale: 0.82,
    angleX: 0.54,
    angleY: -0.37,
    headRotationCosine: Math.cos(0.13),
    headRotationSine: Math.sin(0.13),
    bodyBreathOffset: 1.1,
    headBreathOffset: 0.4,
    ...neutralTorsoPose(),
  }
  let binds = 0
  let uploads = 0
  let uploaded: Float32Array | null = null
  const gl = {
    ARRAY_BUFFER: 1,
    bindBuffer() {
      binds += 1
    },
    bufferSubData(_target: number, _offset: number, data: Float32Array) {
      uploads += 1
      uploaded = data
    },
  } as unknown as WebGL2RenderingContext

  deformCollarClipMesh(clip, pose, 0.95)
  assert.notDeepEqual(clip.deformed, rest)
  assert.deepEqual({ binds, uploads }, { binds: 0, uploads: 0 })

  uploadCollarClipMesh(gl, clip)
  assert.deepEqual({ binds, uploads }, { binds: 1, uploads: 1 })
  assert.equal(uploaded, clip.deformed)
})

test('applies the collar torso shell to the neck aperture mesh', () => {
  const rest = new Float32Array([80, 90, 120, 90, 84, 128, 116, 128])
  const baseline = clipMesh(rest)
  const projected = clipMesh(rest)
  const pose: CollarMotionPose = {
    neckPivotX: 100,
    neckPivotY: 150,
    neckFollowTop: 82,
    neckFollowSpan: 70,
    faceCenterY: 92,
    faceScale: 0.82,
    angleX: 0.54,
    angleY: -0.37,
    headRotationCosine: Math.cos(0.13),
    headRotationSine: Math.sin(0.13),
    bodyBreathOffset: 1.1,
    headBreathOffset: 0.4,
    ...neutralTorsoPose(),
  }
  deformCollarClipMesh(baseline, pose, 0.95)

  const torsoProfile = {
    enabled: true,
    blend: 1,
    centerX: 100,
    radiusX: 80,
    radiusZ: 48,
  }
  const torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.2),
    yawSine: Math.sin(0.2),
  }
  const projectedPose = {
    ...pose,
    torsoProfile,
    torsoShellRotation,
    torsoShellBlend: 0.5,
  }
  deformCollarClipMesh(projected, projectedPose, 0.95)

  for (let index = 0; index < rest.length; index += 2) {
    const headBlend = collarNeckHeadBlend(rest[index + 1], pose)
    const expectedOffset = anime25DTorsoShellOffsetX(
      baseline.deformed[index],
      torsoProfile,
      torsoShellRotation,
      0.5 * (1 - headBlend),
    )
    assert.ok(
      Math.abs(
        projected.deformed[index] - baseline.deformed[index] - expectedOffset,
      ) < 1e-5,
    )
  }
})

test('uses one monotonic vertical follow field across the high collar', () => {
  const pose = { neckFollowTop: 82, neckFollowSpan: 70 }
  const top = collarNeckHeadBlend(82, pose)
  const middle = collarNeckHeadBlend(117, pose)
  const bottom = collarNeckHeadBlend(152, pose)

  assert.equal(top, 1)
  assert.ok(middle > bottom && middle < top)
  assert.equal(bottom, 0)
})

test('closes the alpha contour at the neck aperture', () => {
  const width = 40
  const height = 40
  const rgba = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const halfGap = y < 16 ? Math.max(2, 10 - Math.floor(y / 2)) : 0
    for (let x = 2; x < width - 2; x += 1) {
      if (halfGap > 0 && x > 20 - halfGap && x < 20 + halfGap) continue
      rgba[(y * width + x) * 4 + 3] = 255
    }
  }
  const model = buildFrontCollarContactModel(rgba, width, height, {
    x: 100,
    y: 200,
    w: width,
    h: height,
  })
  assert.ok(model?.closurePoint)
  assert.deepEqual(model.closurePoint, { x: 120.5, y: 216.5 })

  const gl = meshGl()
  const clip = createCollarClipMesh(
    gl,
    {} as WebGLProgram,
    model,
    playbackLayer('neck', 95, 180, 50, 60),
    playbackLayer('collar-front', 100, 200, 40, 40),
  )
  assert.equal(clip.rest.at(-1), model.closurePoint.y)
})

function playbackLayer(
  role: string,
  x: number,
  y: number,
  w: number,
  h: number,
): Anime25DPlaybackLayer {
  return {
    role,
    x,
    y,
    w,
    h,
    atlas: { x: 0.1, y: 0.2, w: 0.3, h: 0.4 },
  } as Anime25DPlaybackLayer
}

function clipMesh(rest: Float32Array): CollarClipMesh {
  return {
    rest,
    deformed: rest.slice(),
    vao: {} as WebGLVertexArrayObject,
    vertexBuffer: {} as WebGLBuffer,
    uvBuffer: {} as WebGLBuffer,
    indexBuffer: {} as WebGLBuffer,
    indexCount: 6,
  }
}

function neutralTorsoPose(): Pick<
  CollarMotionPose,
  'torsoProfile' | 'torsoShellRotation' | 'torsoShellBlend'
> {
  return {
    torsoProfile: {
      enabled: false,
      blend: 0,
      centerX: 100,
      radiusX: 80,
      radiusZ: 48,
    },
    torsoShellRotation: { active: false, yawCosine: 1, yawSine: 0 },
    torsoShellBlend: 0,
  }
}

function meshGl(): WebGL2RenderingContext {
  return {
    ARRAY_BUFFER: 1,
    ELEMENT_ARRAY_BUFFER: 2,
    DYNAMIC_DRAW: 3,
    STATIC_DRAW: 4,
    FLOAT: 5,
    createVertexArray: () => ({}) as WebGLVertexArrayObject,
    createBuffer: () => ({}) as WebGLBuffer,
    getAttribLocation: () => 0,
    bindVertexArray: () => undefined,
    bindBuffer: () => undefined,
    bufferData: () => undefined,
    enableVertexAttribArray: () => undefined,
    vertexAttribPointer: () => undefined,
  } as unknown as WebGL2RenderingContext
}
