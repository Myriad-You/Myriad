import type { Anime25DAttachmentPixels } from './layerAttachment'
import type { Anime25DSecondaryDeformationFrame } from './secondaryDeformation'
import type {
  Anime25DPlayback,
  Anime25DPlaybackAnchors,
  Anime25DPlaybackLayer,
} from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  deriveGeometryChestProfile,
  resolveChestSpatialField,
} from './chestPhysics'
import { IDENTITY_DRIVER } from './driver'
import {
  bindAnime25DLayerAttachment,
  writeAnime25DAttachmentTransform,
} from './layerAttachment'
import { resolveAnime25DLayerDeformationPolicy } from './layerDeformationPolicy'
import {
  compileAnime25DGpuLayers,
  disposeAnime25DGpuLayers,
} from './layerGpuBinding'
import {
  createAnime25DSecondaryDeformationBinding,
  deformAnime25DSecondaryPoint,
} from './secondaryDeformation'
import {
  anime25DShellModeForLayer,
  writeAnime25DShellRotation,
} from './shellDeformation'
import { deriveAnime25DShellProfile } from './shellProfile'
import {
  anime25DTorsoShellModeForLayer,
  stepAnime25DTorsoShellRotation,
} from './torsoDeformation'

const anchors: Anime25DPlaybackAnchors = {
  face: { x0: 313, y0: 173, x1: 699, y1: 636, cx: 511, cy: 386 },
  neckPivot: { x: 524, y: 693 },
  neckTop: 545,
  neckBottom: 719,
  bodyPivot: { x: 524, y: 1340 },
  faceScale: 1.159,
  mouth: { x0: 490, x1: 542, y0: 558, y1: 571, cx: 516, cy: 566 },
}
const face = layer('face', 'head', 313, 173, 386, 463)
const neck = layer('neck', 'body', 449, 543, 150, 179)
const topwear = layer('topwear', 'body', 208, 639, 739, 671)
const necklace = layer('neckwear', 'body', 444, 681, 160, 155)
const hosts = [face, neck, topwear].map(host)
const source = {
  anchors,
  layers: [face, neck, topwear],
  pixelCanvas: { width: 1024, height: 1365 },
}
const shell = deriveAnime25DShellProfile(source)
const chest = deriveGeometryChestProfile(source)

test('neckwear and unknown body ornaments ride the garment, not a separate projection', () => {
  for (const decoration of [
    necklace,
    { ...necklace, name: 'brooch', role: 'unknown' },
  ]) {
    const attachment = bindAnime25DLayerAttachment(
      decoration,
      hosts,
      anchors,
      null,
      1024,
    )
    assert.ok(attachment)
    assert.equal(attachment.hostName, 'topwear')
    const matrix = new Float32Array(9)
    for (const yaw of [-1, -0.5, 0, 0.5, 1]) {
      const pose = frame(yaw)
      writeAnime25DAttachmentTransform(attachment, pose, matrix)
      const expected = { x: attachment.x, y: attachment.y }
      deformAnime25DSecondaryPoint(
        expected,
        expected.x,
        expected.y,
        0,
        hosts[2].secondaryDeformation,
        pose,
      )
      const actual = transform(matrix, attachment.x, attachment.y)
      assert.ok(
        Math.hypot(actual.x - expected.x, actual.y - expected.y) < 0.0001,
      )
      if (Math.abs(yaw) === 1) assert.ok(Math.abs(actual.x - attachment.x) > 10)
    }
  }
})

test('rigid attachment preserves distances, orientation and area at all bounded poses', () => {
  for (const decoration of [
    necklace,
    layer('eyewear', 'head', 360, 430, 300, 80),
    layer('earwear', 'head', 320, 470, 35, 200),
  ]) {
    const attachment = bindAnime25DLayerAttachment(
      decoration,
      hosts,
      anchors,
      null,
      1024,
    )
    assert.ok(attachment)
    const matrix = new Float32Array(9)
    for (let i = 0; i <= 40; i++) {
      const pose = frame(i / 20 - 1, Math.sin(i) * 0.2, 0.6)
      writeAnime25DAttachmentTransform(attachment, pose, matrix)
      assert.ok([...matrix].every(Number.isFinite))
      assert.ok(Math.abs(Math.hypot(matrix[0], matrix[1]) - 1) < 1e-6)
      assert.ok(Math.abs(Math.hypot(matrix[3], matrix[4]) - 1) < 1e-6)
      assert.ok(
        Math.abs(matrix[0] * matrix[4] - matrix[1] * matrix[3] - 1) < 1e-6,
      )
      const a = transform(matrix, decoration.x, decoration.y)
      const b = transform(
        matrix,
        decoration.x + decoration.w,
        decoration.y + decoration.h,
      )
      assert.ok(
        Math.abs(
          Math.hypot(a.x - b.x, a.y - b.y) -
            Math.hypot(decoration.w, decoration.h),
        ) < 0.0001,
      )
    }
  }
})

test('neutral pose is identity; a neck-local item attaches to neck, never a fabricated collar', () => {
  const choker = layer('neckwear', 'body', 470, 665, 100, 20)
  const attachment = bindAnime25DLayerAttachment(
    choker,
    hosts,
    anchors,
    null,
    1024,
  )
  assert.ok(attachment)
  assert.equal(attachment.hostName, 'neck')
  const matrix = new Float32Array(9)
  writeAnime25DAttachmentTransform(attachment, frame(0), matrix)
  for (const [i, expected] of [1, 0, 0, 0, 1, 0, 0, 0, 1].entries())
    assert.ok(Math.abs(matrix[i] - expected) < 1e-6)
  assert.equal(
    bindAnime25DLayerAttachment(necklace, [], anchors, null, 1024),
    null,
  )
  assert.equal(
    bindAnime25DLayerAttachment(topwear, hosts, anchors, null, 1024),
    null,
  )
  assert.equal(
    bindAnime25DLayerAttachment(
      { ...necklace, fade: 'eyeCry' },
      hosts,
      anchors,
      null,
      1024,
    ),
    null,
  )
})

test('decorations use a static mesh even when the parent uses nonlinear geometry', () => {
  const policy = resolveAnime25DLayerDeformationPolicy({
    baseRole: 'neckwear',
    fade: null,
    hairPhysics: false,
    hasBangWeights: false,
    hasFrontHairParallax: false,
    hasCollarContact: false,
    shellDeformation: true,
    rigidAttachment: true,
  })
  assert.equal(policy.shaderGlobalTransform, true)
  assert.equal(policy.localDynamic, false)
  assert.deepEqual(policy.deformationExtensions, ['rigid-surface-attachment'])
})

test('transparent padding cannot move an ornament or its visible ear root', () => {
  for (const role of ['neckwear', 'earwear']) {
    const tight = layer(
      role,
      role === 'earwear' ? 'head' : 'body',
      490,
      740,
      20,
      40,
    )
    const padded = { ...tight, x: 450, y: 710, w: 100, h: 100 }
    const read = (source: Anime25DPlaybackLayer) =>
      source === padded
        ? pixels(100, 100, (x, y) => x >= 40 && x < 60 && y >= 30 && y < 70)
        : pixels(20, 40, () => true)
    const a = bindAnime25DLayerAttachment(
      tight,
      hosts,
      anchors,
      null,
      1024,
      read,
    )!
    const b = bindAnime25DLayerAttachment(
      padded,
      hosts,
      anchors,
      null,
      1024,
      read,
    )!
    assert.equal(a.x, b.x)
    assert.equal(a.y, b.y)
    if (role === 'earwear') assert.ok(a.y < tight.y + tight.h * 0.15)
    const ma = new Float32Array(9)
    const mb = new Float32Array(9)
    for (const yaw of [-1, 0, 1]) {
      writeAnime25DAttachmentTransform(a, frame(yaw), ma)
      writeAnime25DAttachmentTransform(b, frame(yaw), mb)
      assert.deepEqual(ma, mb)
    }
  }
})

test('host selection uses real alpha support and respects explicit sides', () => {
  const empty = { ...topwear, name: 'empty-garment' }
  const solid = { ...topwear, name: 'supported-garment', side: 'L' as const }
  const wrongSide = { ...topwear, name: 'other-side', side: 'R' as const }
  const ornament = { ...necklace, side: 'L' as const }
  const read = (source: Anime25DPlaybackLayer) =>
    pixels(20, 20, () => source !== empty)
  const attachment = bindAnime25DLayerAttachment(
    ornament,
    [host(empty), host(wrongSide), host(solid)],
    anchors,
    null,
    1024,
    read,
  )
  assert.equal(attachment?.hostName, 'supported-garment')
  const detached = bindAnime25DLayerAttachment(
    ornament,
    [host(empty)],
    anchors,
    null,
    1024,
    read,
  )
  assert.equal(
    detached?.hostName,
    'empty-garment',
    'no-contact hanging art retains its semantic mount',
  )
})

test('single-pixel roots stay finite and collars never enter attachment sampling', () => {
  const decoration = layer('earwear', 'head', 320, 470, 1, 1)
  const attachment = bindAnime25DLayerAttachment(
    decoration,
    hosts,
    anchors,
    null,
    1024,
    () => pixels(1, 1, () => true),
  )!
  assert.ok(Number.isFinite(attachment.x) && Number.isFinite(attachment.y))
  for (const role of ['collar-front', 'collar-back', 'topwear', 'neck']) {
    assert.equal(
      bindAnime25DLayerAttachment(
        { ...necklace, role },
        hosts,
        anchors,
        null,
        1024,
        () => {
          throw new Error('must not sample garment topology')
        },
      ),
      null,
    )
  }
})

function pixels(
  width: number,
  height: number,
  opaque: (x: number, y: number) => boolean,
): Anime25DAttachmentPixels {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++)
      data[(y * width + x) * 4 + 3] = opaque(x, y) ? 255 : 0
  }
  return { width, height, pixels: data }
}

test('GPU compilation actually binds independent accessories to their surfaces', () => {
  const eyewear = layer('eyewear', 'head', 360, 430, 300, 80)
  const playback = {
    ...source,
    layers: [...source.layers, necklace, eyewear],
  } as Anime25DPlayback
  const gl = {
    ARRAY_BUFFER: 1,
    ELEMENT_ARRAY_BUFFER: 2,
    DYNAMIC_DRAW: 3,
    STATIC_DRAW: 4,
    FLOAT: 5,
    createVertexArray: () => ({}),
    createBuffer: () => ({}),
    getAttribLocation: () => 0,
    bindVertexArray() {},
    bindBuffer() {},
    bufferData() {},
    enableVertexAttribArray() {},
    vertexAttribPointer() {},
    deleteBuffer() {},
    deleteVertexArray() {},
  } as unknown as WebGL2RenderingContext
  const previousDocument = globalThis.document
  let reads = 0
  Object.assign(globalThis, {
    document: {
      createElement: () => ({
        getContext: () => ({
          drawImage() {},
          getImageData(_x: number, _y: number, w: number, h: number) {
            reads++
            return { data: pixels(w, h, () => true).pixels }
          },
        }),
      }),
    },
  })
  let compiled: ReturnType<typeof compileAnime25DGpuLayers> | undefined
  try {
    compiled = compileAnime25DGpuLayers(
      gl,
      {} as WebGLProgram,
      playback,
      shell,
      { ...IDENTITY_DRIVER },
      null,
      { width: 8, height: 8 } as HTMLImageElement,
    )
    assert.equal(compiled.collarClip, null)
    for (const [role, parent] of [
      ['neckwear', 'topwear'],
      ['eyewear', 'face'],
    ]) {
      const drawing = compiled.layers.find((l) => l.source.role === role)
      assert.ok(drawing?.attachment)
      assert.equal(drawing.attachment.hostName, parent)
      assert.equal(drawing.localDynamic, false)
      assert.equal(drawing.deformed, drawing.rest)
      writeAnime25DAttachmentTransform(
        drawing.attachment,
        frame(0.8),
        drawing.layerTransform,
      )
      assert.ok([...drawing.layerTransform].every(Number.isFinite))
    }
    assert.equal(
      reads,
      5,
      'neck seam, ornaments and shared hosts are sampled once at binding, never at frame time',
    )
  } finally {
    if (compiled) disposeAnime25DGpuLayers(gl, compiled)
    Object.assign(globalThis, { document: previousDocument })
  }
})

test('attachment includes shader-owned host motion when shell projection is disabled', () => {
  const shaderHost = host(face)
  shaderHost.secondaryDeformation.shaderGlobalTransform = true
  const decoration = layer('eyewear', 'head', 360, 430, 300, 80)
  const attachment = bindAnime25DLayerAttachment(
    decoration,
    [shaderHost],
    anchors,
    null,
    1024,
  )
  assert.ok(attachment)
  const pose = frame(0.8, 0.15)
  pose.shellProfile = { ...pose.shellProfile, enabled: false }
  pose.shellBlend = 0
  const matrix = new Float32Array(9)
  writeAnime25DAttachmentTransform(attachment, pose, matrix)
  const actual = transform(matrix, attachment.x, attachment.y)
  const expected = { x: attachment.x, y: attachment.y }
  deformAnime25DSecondaryPoint(
    expected,
    expected.x,
    expected.y,
    0,
    { ...shaderHost.secondaryDeformation, shaderGlobalTransform: false },
    pose,
  )
  assert.ok(Math.hypot(actual.x - expected.x, actual.y - expected.y) < 0.0001)
  assert.ok(Math.hypot(actual.x - attachment.x, actual.y - attachment.y) > 1)
})

test('ordinary neck root takes torso yaw while the head end remains free', () => {
  const binding = hosts[1].secondaryDeformation
  assert.equal(binding.torsoShellMode, 'neck')
  const on = frame(1)
  const off = { ...on, torsoShellBlend: 0 }
  for (const y of [on.neckFollowTop, anchors.neckBottom]) {
    const a = { x: anchors.neckPivot.x, y }
    const b = { ...a }
    deformAnime25DSecondaryPoint(a, a.x, y, 0, binding, on)
    deformAnime25DSecondaryPoint(b, b.x, y, 0, binding, off)
    if (y === anchors.neckBottom) assert.ok(a.x - b.x > 10)
    else assert.ok(Math.abs(a.x - b.x) < 1e-8)
  }
})

function layer(
  role: string,
  group: 'head' | 'body',
  x: number,
  y: number,
  w: number,
  h: number,
): Anime25DPlaybackLayer {
  return {
    name: role,
    role,
    group,
    x,
    y,
    w,
    h,
    depth: 1,
    phys: null,
    fade: null,
    side: null,
    strands: [],
    atlas: { x: 0, y: 0, w: 1, h: 1 },
  }
}
function host(source: Anime25DPlaybackLayer) {
  return {
    source,
    secondaryDeformation: createAnime25DSecondaryDeformationBinding({
      source,
      baseRole: source.role,
      shaderGlobalTransform: false,
      collarContact: false,
      frontHair: false,
      frontHairParallaxScale: null,
      chestWeights: null,
      bangWeights: null,
      strandWeights: null,
      alongStrand: null,
      springs: null,
      shellMode: anime25DShellModeForLayer(source),
      torsoShellMode: anime25DTorsoShellModeForLayer(source),
    }),
  }
}
function transform(m: Float32Array, x: number, y: number) {
  return { x: m[0] * x + m[3] * y + m[6], y: m[1] * x + m[4] * y + m[7] }
}
function frame(
  yaw: number,
  roll = 0,
  breath = 0,
  pitch = 0,
): Anime25DSecondaryDeformationFrame {
  const rotation = { active: false, yawCosine: 1, yawSine: 0 }
  stepAnime25DTorsoShellRotation({ value: yaw * 0.45 }, yaw, 0, 0, rotation)
  const headRotation = { ...rotation, pitchCosine: 1, pitchSine: 0 }
  writeAnime25DShellRotation(yaw, pitch, headRotation)
  const top = anchors.face.y1 + anchors.faceScale * 5
  return {
    expression: { ...IDENTITY_DRIVER, angleX: yaw, angleY: pitch },
    faceScale: anchors.faceScale,
    headAngleY: pitch,
    headRotationCosine: Math.cos(roll),
    headRotationSine: Math.sin(roll),
    neckPivotX: anchors.neckPivot.x,
    neckPivotY: anchors.neckPivot.y,
    neckBottom: anchors.neckBottom,
    neckFollowTop: top,
    neckFollowSpan: anchors.neckBottom - top,
    faceCenterY: anchors.face.cy,
    bodyBreathOffset: breath * 2,
    headBreathOffset: breath,
    specialHeadOffset: 0,
    highCollar: false,
    breath,
    armSwing: 0,
    chestCenterX: chest.centerX,
    chestRegionCenterY: chest.centerY,
    chestMotionCenterY: chest.centerY,
    chestRadiusY: chest.radiusY,
    inverseChestRadiusX: 1 / chest.radiusX,
    inverseChestRadiusY: 1 / chest.radiusY,
    chestOffsetX: 0,
    chestOffsetY: 0,
    chestField: resolveChestSpatialField(chest),
    chestVolumeScale: 1,
    shellProfile: shell,
    shellBlend: shell.blend,
    shellActivation: 1,
    shellRotation: headRotation,
    torsoProfile: shell.torso,
    torsoChestShape: null,
    torsoShellBlend: shell.blend * shell.torso.blend,
    torsoShellRotation: rotation,
  }
}

test('125 combined poses keep the neck mesh unfolded and its lower join near the garment', () => {
  const neckBinding = hosts[1].secondaryDeformation
  const bodyBinding = hosts[2].secondaryDeformation
  const at = (x: number, y: number, binding: typeof neckBinding,
    pose: Anime25DSecondaryDeformationFrame) => {
    const point = { x, y }
    deformAnime25DSecondaryPoint(point, x, y, 0, binding, pose)
    assert.ok(Number.isFinite(point.x) && Number.isFinite(point.y))
    return point
  }
  for (const yaw of [-1, -0.5, 0, 0.5, 1]) {
    for (const pitch of [-1, -0.5, 0, 0.5, 1]) {
      for (const roll of [-0.35, -0.175, 0, 0.175, 0.35]) {
        const pose = frame(yaw, roll, 0.5, pitch)
        for (let row = 0; row < 10; row++) {
          for (let column = 0; column < 6; column++) {
            const x = neck.x + column * neck.w / 6
            const y = neck.y + row * neck.h / 10
            const a = at(x, y, neckBinding, pose)
            const b = at(x + neck.w / 6, y, neckBinding, pose)
            const c = at(x, y + neck.h / 10, neckBinding, pose)
            const area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
            assert.ok(area > 0, `folded neck at ${yaw},${pitch},${roll}`)
          }
        }
        for (const x of [neck.x + neck.w * 0.25, anchors.neckPivot.x, neck.x + neck.w * 0.75]) {
          const y = anchors.neckBottom - 10
          const a = at(x, y, neckBinding, pose)
          const b = at(x, y, bodyBinding, pose)
          assert.ok(Math.hypot(a.x - b.x, a.y - b.y) < neck.w * 0.04,
            `join drift at ${yaw},${pitch},${roll}: ${Math.hypot(a.x-b.x,a.y-b.y)}`)
        }
      }
    }
  }
})
