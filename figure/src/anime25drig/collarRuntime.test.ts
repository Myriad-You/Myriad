import type { CollarClipMesh, CollarMotionPose } from './collarRuntime'
import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { buildFrontCollarContactModel } from './collarContact'
import {
  collarNeckHeadBlend,
  createCollarClipMesh,
  deformCollarClipMesh,
  frontCollarNeckWeights,
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
    torsoNeckOffsetX: 17,
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
        projected.deformed[index] - baseline.deformed[index] - expectedOffset - 17 * headBlend,
      ) < 1e-5,
    )
  }
})

test('turns a figure\'s neck aperture at the neck\'s depth, as the neck itself turns', () => {
  // Rows at the neck's foot, where only the body moves the opening.
  const rest = new Float32Array([84, 152, 116, 152, 84, 160, 116, 160])
  const pose: CollarMotionPose = {
    neckPivotX: 100,
    neckPivotY: 150,
    neckFollowTop: 82,
    neckFollowSpan: 70,
    faceCenterY: 92,
    faceScale: 0.82,
    angleX: 0,
    angleY: 0,
    headRotationCosine: 1,
    headRotationSine: 0,
    bodyBreathOffset: 0,
    headBreathOffset: 0,
    ...neutralTorsoPose(),
    torsoProfile: { enabled: true, blend: 1, centerX: 100, radiusX: 80, radiusZ: 48, neckDepth: 0.25 },
    torsoShellRotation: { active: true, yawCosine: Math.cos(0.4), yawSine: Math.sin(0.4) },
    torsoShellBlend: 1,
  }
  const still = clipMesh(rest)
  deformCollarClipMesh(still, { ...pose, torsoShellBlend: 0 }, 1)
  const turned = clipMesh(rest)
  deformCollarClipMesh(turned, pose, 1)
  for (let index = 0; index < rest.length; index += 2) {
    const travel = turned.deformed[index] - still.deformed[index]
    // The neck's own vertices at that height: torsoShellOffsetX at the figure's front depth there.
    const neckTravel = anime25DTorsoShellOffsetX(
      still.deformed[index],
      pose.torsoProfile,
      pose.torsoShellRotation,
      1 - collarNeckHeadBlend(rest[index + 1], pose),
      0.25 + 0.75 * smoothstepForTest((rest[index + 1] - 152) / (220 - 152)),
    )
    assert.ok(Math.abs(travel - neckTravel) < 1e-4, `vertex ${index / 2}: ${travel} vs ${neckTravel}`)
    const trunkTravel = anime25DTorsoShellOffsetX(still.deformed[index], pose.torsoProfile, pose.torsoShellRotation, 1)
    assert.ok(Math.abs(travel) < Math.abs(trunkTravel) * 0.6)
  }
})

function smoothstepForTest(value: number): number {
  const t = Math.min(1, Math.max(0, value))
  return t * t * (3 - 2 * t)
}

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

test('a deep V the closure search does not reach keeps the neck showing down through it', () => {
  const width = 40
  const height = 40
  const rgba = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    // The gap narrows to its point only at the collar's bottom row.
    const halfGap = Math.max(1, 10 - Math.floor(y / 4))
    for (let x = 2; x < width - 2; x += 1) {
      if (x > 20 - halfGap && x < 20 + halfGap) continue
      rgba[(y * width + x) * 4 + 3] = 255
    }
  }
  const model = buildFrontCollarContactModel(rgba, width, height, {
    x: 100,
    y: 200,
    w: width,
    h: height,
  })
  assert.ok(model)
  assert.equal(model.closurePoint, null)
  const clip = createCollarClipMesh(
    meshGl(),
    {} as WebGLProgram,
    model,
    playbackLayer('neck', 95, 180, 50, 58),
    playbackLayer('collar-front', 100, 200, 40, 40),
  )
  const lastContactY = model.handles[model.contactPairs.at(-1)! * 2 + 1]
  assert.ok(lastContactY < 230, 'the contacts stop part way down the opening')
  assert.equal(clip.rest.at(-1), 238, 'on down to the neck bottom, above the collar bottom')
  assert.equal(clip.rest.at(-3), 238)
  assert.equal(clip.indexCount, (clip.rest.length / 4 - 1) * 6)
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
  'torsoProfile' | 'torsoShellRotation' | 'torsoShellBlend' | 'torsoNeckOffsetX' | 'neckBottom' | 'chestRegionCenterY'
> {
  return {
    neckBottom: 152,
    chestRegionCenterY: 220,
    torsoProfile: {
      enabled: false,
      blend: 0,
      centerX: 100,
      radiusX: 80,
      radiusZ: 48,
    },
    torsoShellRotation: { active: false, yawCosine: 1, yawSine: 0 },
    torsoShellBlend: 0,
    torsoNeckOffsetX: 0,
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

test('a front collar takes the neck\'s motion over the neck and less of it out beside it', () => {
  // A neck 20 wide (x 40..59) on a 100 px canvas, drawn 1:1.
  const neck = { role: 'neck', x: 40, y: 0, w: 20, h: 40 } as Anime25DPlaybackLayer
  const pixels = new Uint8ClampedArray(20 * 40 * 4)
  for (let p = 0; p < 20 * 40; p++) pixels[p * 4 + 3] = 255
  const rest = Float32Array.from([50, 20, 60, 20, 65, 20, 70, 20, 90, 20])
  const weights = frontCollarNeckWeights(rest, [neck], () => ({ pixels, width: 20, height: 40 }))!
  assert.equal(weights[0], 1, 'over the neck')
  assert.ok(weights[2] > 0.3 && weights[2] < 0.7, `half the fade out: ${weights[2]}`)
  assert.equal(weights[3], 0, 'half the neck\'s width out')
  assert.equal(weights[4], 0)
  assert.ok(weights[1] >= weights[2] && weights[2] >= weights[3])
  assert.equal(frontCollarNeckWeights(rest, [], () => null), null)
})
