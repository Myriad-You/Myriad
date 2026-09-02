import type { Anime25DLayerSpringBinding } from './layerBinding'
import type {
  Anime25DSecondaryDeformationBinding,
  Anime25DSecondaryDeformationFrame,
} from './secondaryDeformation'
import type { Anime25DPlaybackLayer, Anime25DShellProfile } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  chestDeformationWeight,
  resolveChestSpatialField,
} from './chestPhysics'
import {
  BODY_HEAD_FOLLOW,
  FRONT_COLLAR_FLEX_REGION,
  FRONT_COLLAR_HEAD_FOLLOW,
  FRONT_COLLAR_INNER_REGION,
  HIGH_COLLAR_NECK_FOLLOW_POWER,
} from './collarRuntime'
import { IDENTITY_DRIVER } from './driver'
import {
  createAnime25DSecondaryDeformationBinding,
  deformAnime25DHairPoint,
  deformAnime25DSecondaryPoint,
} from './secondaryDeformation'
import { writeAnime25DShellRotation } from './shellDeformation'
import { deformAnime25DTorsoShellPoint } from './torsoDeformation'

const VERTEX_COUNT = 48

test('binds stable secondary roles and optional geometry fields once', () => {
  const topwear = secondaryBinding('topwear', 'body', false)
  assert.equal(topwear.topwear, true)
  assert.equal(topwear.head, false)
  assert.equal(topwear.frontCollar, false)

  const collar = secondaryBinding('collar_front', 'body', false, true)
  assert.equal(collar.frontCollar, true)
  assert.equal(collar.collarContact, true)

  const rearCollar = secondaryBinding('collar_back', 'body', false)
  assert.equal(rearCollar.rearCollar, true)
  assert.equal(rearCollar.frontCollar, false)

  const hair = secondaryBinding('front hair', 'head', false, false, true)
  assert.equal(hair.head, true)
  assert.equal(hair.frontHair, true)
  assert.equal(hair.springs?.length, 3)
})

test('secondary and hair stages match the frozen player branches', () => {
  const bindings = secondaryBindings()
  for (let frameIndex = 0; frameIndex < 120; frameIndex += 1) {
    const progress = frameIndex / 119
    const frame = secondaryFrame(progress, frameIndex)
    for (const binding of bindings) {
      for (let row = 0; row <= 5; row += 1) {
        for (let column = 0; column <= 7; column += 1) {
          const vertex = row * 8 + column
          const source = binding.source
          const restX = source.x + (source.w * column) / 7
          const restY = source.y + (source.h * row) / 5
          const initialX = restX + Math.sin(frameIndex * 0.17 + vertex) * 1.3
          const initialY = restY + Math.cos(frameIndex * 0.13 + vertex) * 1.1
          const actual = { x: initialX, y: initialY }
          const expected = { ...actual }
          legacyDeformSecondaryPoint(
            expected,
            restX,
            restY,
            vertex,
            binding,
            frame,
          )
          legacyDeformHairPoint(expected, vertex, binding, frame)
          deformAnime25DSecondaryPoint(
            actual,
            restX,
            restY,
            vertex,
            binding,
            frame,
          )
          deformAnime25DHairPoint(actual, vertex, binding, frame)
          assert.deepEqual(
            actual,
            expected,
            `${binding.baseRole} frame ${frameIndex} vertex ${row}:${column}`,
          )
        }
      }
    }
  }
})

test('ramps the shell into the secondary deformation path from exact legacy output', () => {
  const binding = {
    ...secondaryBinding('face', 'head', false),
    shellMode: 'head' as const,
  }
  const frame = secondaryFrame(0.46, 17)
  frame.expression = { ...frame.expression, angleX: 0.8 }
  frame.headAngleY = -0.35
  frame.headRotationCosine = 1
  frame.headRotationSine = 0
  frame.bodyBreathOffset = 0
  frame.headBreathOffset = 0
  frame.specialHeadOffset = 0
  frame.shellProfile = shellProfile()
  frame.shellRotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
    pitchCosine: 1,
    pitchSine: 0,
  }
  writeAnime25DShellRotation(
    frame.expression.angleX,
    frame.headAngleY,
    frame.shellRotation,
  )

  const restX = 124
  const restY = 132
  const legacy = { x: restX, y: restY }
  legacyDeformSecondaryPoint(legacy, restX, restY, 0, binding, frame)

  frame.shellActivation = 0
  frame.shellBlend = 0
  const rampStart = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(rampStart, restX, restY, 0, binding, frame)
  assert.deepEqual(rampStart, legacy)

  frame.shellActivation = 1
  frame.shellBlend = frame.shellProfile.blend
  const active = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(active, restX, restY, 0, binding, frame)
  assert.equal(Number.isFinite(active.x), true)
  assert.equal(Number.isFinite(active.y), true)
  assert.notDeepEqual(active, legacy)
  assert.ok(Math.hypot(active.x - legacy.x, active.y - legacy.y) < 80)
})

test('fully pinned hairline vertices reject bang and spring displacement', () => {
  const unpinned = secondaryBinding('front hair', 'head', false, false, true)
  const hairlinePinWeights = new Float32Array(VERTEX_COUNT)
  hairlinePinWeights[VERTEX_COUNT - 1] = 1
  const pinned = { ...unpinned, hairlinePinWeights }
  const frame = secondaryFrame(0.63, 21)
  frame.shellActivation = 1
  const vertex = VERTEX_COUNT - 1
  const rest = { x: 132, y: 98 }
  const moving = { ...rest }
  const fixed = { ...rest }

  deformAnime25DHairPoint(moving, vertex, unpinned, frame)
  deformAnime25DHairPoint(fixed, vertex, pinned, frame)

  assert.notDeepEqual(moving, rest)
  assert.deepEqual(fixed, rest)
})

test('composes torso volume after existing topwear chest deformation', () => {
  const binding = {
    ...secondaryBinding('topwear', 'body', false),
    torsoShellMode: 'full' as const,
  }
  const frame = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  frame.torsoProfile = profile
  frame.torsoShellBlend = 0.25
  frame.torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }
  frame.torsoChestShape = {
    centerX: 151,
    centerY: 126,
    radiusX: 60,
    radiusY: 48,
    scale: 0.8,
    field: frame.chestField,
  }
  const restX = 151
  const restY = 126
  const vertex = 19
  const expected = { x: restX, y: restY }
  legacyDeformSecondaryPoint(expected, restX, restY, vertex, binding, frame)
  deformAnime25DTorsoShellPoint(
    expected,
    profile,
    frame.torsoShellRotation,
    frame.torsoShellBlend,
    frame.torsoChestShape,
    binding.chestWeights?.[vertex] ?? 1,
    frame.chestVolumeScale,
  )

  const actual = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(actual, restX, restY, vertex, binding, frame)
  assert.deepEqual(actual, expected)
  assert.notEqual(actual.x, restX)
})

test('keeps chest volume off collar layers', () => {
  const binding = {
    ...secondaryBinding('collar_front', 'body', false),
    torsoShellMode: 'collar' as const,
  }
  const withoutChest = secondaryFrame(0.58, 23)
  const withChest = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  for (const frame of [withoutChest, withChest]) {
    frame.torsoProfile = profile
    frame.torsoShellBlend = 0.25
    frame.torsoShellRotation = {
      active: true,
      yawCosine: Math.cos(0.24),
      yawSine: Math.sin(0.24),
    }
  }
  withChest.torsoChestShape = {
    centerX: 151,
    centerY: 126,
    radiusX: 60,
    radiusY: 48,
    scale: 0.8,
  }

  const rest = { x: 151, y: 126 }
  assert.deepEqual(
    deformSecondary({ ...rest }, binding, withChest),
    deformSecondary({ ...rest }, binding, withoutChest),
  )
})

test('uses the live chest center for both motion and torso volume', () => {
  const binding = {
    ...secondaryBinding('topwear', 'body', false),
    torsoShellMode: 'full' as const,
  }
  const aligned = secondaryFrame(0.58, 23)
  const stale = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  const rotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }
  for (const frame of [aligned, stale]) {
    frame.torsoProfile = profile
    frame.torsoShellBlend = 0.25
    frame.torsoShellRotation = rotation
    frame.chestMotionCenterY = 154
    frame.torsoChestShape = {
      centerX: 151,
      centerY: 154,
      radiusX: 60,
      radiusY: 48,
      scale: 0.8,
      field: frame.chestField,
    }
  }
  stale.torsoChestShape!.centerY = 106
  const rest = { x: 151, y: 154 }
  const alignedPoint = { ...rest }
  const stalePoint = { ...rest }
  deformAnime25DSecondaryPoint(
    alignedPoint,
    rest.x,
    rest.y,
    19,
    binding,
    aligned,
  )
  deformAnime25DSecondaryPoint(stalePoint, rest.x, rest.y, 19, binding, stale)
  assert.notEqual(alignedPoint.x, stalePoint.x)
})

test('fades fallback front-collar torso motion from body edge to neck seam', () => {
  const binding = {
    ...secondaryBinding('collar_front', 'body', false),
    torsoShellMode: 'collar' as const,
  }
  const frame = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  frame.torsoProfile = profile
  frame.torsoShellBlend = 0.25
  frame.torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }

  const neckSeamRest = { x: 117, y: 70 }
  const neckSeamLegacy = { ...neckSeamRest }
  legacyDeformSecondaryPoint(
    neckSeamLegacy,
    neckSeamRest.x,
    neckSeamRest.y,
    0,
    binding,
    frame,
  )
  const neckSeamActual = { ...neckSeamRest }
  deformAnime25DSecondaryPoint(
    neckSeamActual,
    neckSeamRest.x,
    neckSeamRest.y,
    0,
    binding,
    frame,
  )
  assert.deepEqual(neckSeamActual, neckSeamLegacy)

  const bodyEdgeRest = { x: 54, y: 224 }
  const bodyEdgeLegacy = { ...bodyEdgeRest }
  legacyDeformSecondaryPoint(
    bodyEdgeLegacy,
    bodyEdgeRest.x,
    bodyEdgeRest.y,
    0,
    binding,
    frame,
  )
  const bodyEdgeActual = { ...bodyEdgeRest }
  deformAnime25DSecondaryPoint(
    bodyEdgeActual,
    bodyEdgeRest.x,
    bodyEdgeRest.y,
    0,
    binding,
    frame,
  )
  assert.notDeepEqual(bodyEdgeActual, bodyEdgeLegacy)
})

test('keeps narrow contact-collar rows coherent at the allowed yaw limit', () => {
  const binding = secondaryBinding('collar_front', 'body', false, true)
  const frame = highCollarFrame()
  const leftOuter = deformSecondary({ x: 498, y: 635.17 }, binding, frame)
  const leftInner = deformSecondary({ x: 500, y: 635.17 }, binding, frame)
  const rightInner = deformSecondary({ x: 599, y: 635.17 }, binding, frame)
  const rightOuter = deformSecondary({ x: 604, y: 635.17 }, binding, frame)

  assert.ok(Math.abs(leftInner.x - leftOuter.x - 2) < 1e-9)
  assert.ok(Math.abs(rightOuter.x - rightInner.x - 5) < 1e-9)
  assert.equal(leftOuter.y, leftInner.y)
  assert.equal(rightInner.y, rightOuter.y)
})

test('front and rear high-collar layers share the same vertical motion field', () => {
  const front = secondaryBinding('collar_front', 'body', false, true)
  const rear = secondaryBinding('collar_back', 'body', false)
  const frame = highCollarFrame()
  const rest = { x: 552, y: 652.17 }

  assert.deepEqual(
    deformSecondary(rest, front, frame),
    deformSecondary(rest, rear, frame),
  )
})

test('fades split-collar torso volume with the inverse neck-follow field', () => {
  const binding = {
    ...secondaryBinding('collar_front', 'body', false, true),
    torsoShellMode: 'collar' as const,
  }
  const withoutTorso = highCollarFrame()
  const withTorso = highCollarFrame()
  const profile = shellProfile().torso
  assert.ok(profile)
  withTorso.torsoProfile = profile
  withTorso.torsoShellBlend = 0.25
  withTorso.torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }

  const top = { x: 604, y: withTorso.neckFollowTop }
  assert.deepEqual(
    deformSecondary(top, binding, withTorso),
    deformSecondary(top, binding, withoutTorso),
  )

  const bottom = { x: 604, y: withTorso.neckBottom }
  assert.notDeepEqual(
    deformSecondary(bottom, binding, withTorso),
    deformSecondary(bottom, binding, withoutTorso),
  )
})

function secondaryBindings(): Anime25DSecondaryDeformationBinding[] {
  return [
    secondaryBinding('face', 'head', false),
    secondaryBinding('body', 'body', false),
    secondaryBinding('neck', 'body', false),
    secondaryBinding('collar_front', 'body', false),
    secondaryBinding('topwear', 'body', false),
    secondaryBinding('handwear', 'body', false),
    secondaryBinding('front hair', 'head', false, false, true),
    secondaryBinding('back hair', 'head', false, false, true, false),
    secondaryBinding('accessory', 'head', true),
  ]
}

function highCollarFrame(): Anime25DSecondaryDeformationFrame {
  const frame = secondaryFrame(0.58, 24)
  frame.expression = { ...frame.expression, angleX: 0.38 }
  frame.faceScale = 1.156156
  frame.headAngleY = 0
  frame.headRotationCosine = 1
  frame.headRotationSine = 0
  frame.neckPivotX = 548.9295
  frame.neckPivotY = 684.9667
  frame.neckBottom = 711.6667
  frame.neckFollowTop = 628.44745
  frame.neckFollowSpan = 83.21925
  frame.bodyBreathOffset = 0
  frame.headBreathOffset = 0
  frame.specialHeadOffset = 0
  frame.highCollar = true
  frame.torsoShellBlend = 0
  return frame
}

function deformSecondary(
  rest: { x: number; y: number },
  binding: Anime25DSecondaryDeformationBinding,
  frame: Anime25DSecondaryDeformationFrame,
): { x: number; y: number } {
  const point = { ...rest }
  deformAnime25DSecondaryPoint(point, rest.x, rest.y, 0, binding, frame)
  return point
}

function secondaryBinding(
  baseRole: string,
  group: Anime25DPlaybackLayer['group'],
  shaderGlobalTransform: boolean,
  collarContact = false,
  hair = false,
  frontHair = true,
): Anime25DSecondaryDeformationBinding {
  const source: Anime25DSecondaryDeformationBinding['source'] = {
    role: baseRole.replaceAll('_', '-').replace('front hair', 'front-hair'),
    group,
    depth: group === 'head' ? 0.78 : 0.91,
    x: 54,
    y: 70,
    w: 126,
    h: 154,
  }
  const alongStrand = hair ? new Float32Array(VERTEX_COUNT) : null
  const frontHairParallaxScale = hair ? new Float32Array(VERTEX_COUNT) : null
  const bangWeights =
    hair && frontHair ? new Float32Array(VERTEX_COUNT * 3) : null
  const strandWeights = hair ? new Float32Array(VERTEX_COUNT * 3) : null
  for (let vertex = 0; vertex < VERTEX_COUNT; vertex += 1) {
    const progress = vertex / (VERTEX_COUNT - 1)
    if (alongStrand) alongStrand[vertex] = progress
    if (frontHairParallaxScale) {
      frontHairParallaxScale[vertex] = 0.2 + progress * 0.8
    }
    if (bangWeights) {
      bangWeights[vertex * 3] = 1 - progress
      bangWeights[vertex * 3 + 1] = 0.35 + progress * 0.3
      bangWeights[vertex * 3 + 2] = progress
    }
    if (strandWeights) {
      strandWeights[vertex * 3] = 0.42 - progress * 0.1
      strandWeights[vertex * 3 + 1] = 0.37
      strandWeights[vertex * 3 + 2] = 0.21 + progress * 0.1
    }
  }
  return createAnime25DSecondaryDeformationBinding({
    source,
    baseRole,
    shaderGlobalTransform,
    collarContact,
    frontHair: hair && frontHair,
    frontHairParallaxScale,
    chestWeights:
      baseRole === 'topwear'
        ? Float32Array.from(
            { length: VERTEX_COUNT },
            (_, vertex) => 0.25 + (vertex % 9) * 0.08,
          )
        : null,
    bangWeights,
    strandWeights,
    alongStrand,
    springs: hair ? hairSprings() : null,
  })
}

function hairSprings(): Anime25DLayerSpringBinding[] {
  return [spring(-2.6, 1.4), spring(0.8, -1.1), spring(3.2, 1.9)]
}

function shellProfile(): Anime25DShellProfile {
  return {
    version: 1,
    source: 'authored',
    enabled: true,
    blend: 0.5,
    head: {
      centerX: 117,
      centerY: 139,
      radiusX: 74,
      radiusY: 96,
      radiusZ: 58,
    },
    faceProfile: {
      enabled: true,
      startY: 74,
      endY: 206,
      points: [
        { v: 0, z: 0 },
        { v: 0.25, z: 0.12 },
        { v: 0.5, z: 0.22 },
        { v: 0.75, z: 0.1 },
        { v: 1, z: 0 },
      ],
    },
    hair: {
      centerX: 117,
      centerY: 139,
      radiusX: 82,
      radiusY: 106,
      radiusZ: 62,
      frontGap: 0.18,
      frontBulge: 1,
      backDepth: 0.35,
      crownRound: 1,
      hairlinePin: {
        enabled: true,
        centerX: 0,
        centerY: -0.45,
        halfWidth: 1.1,
        halfHeight: 0.32,
        feather: 0.06,
      },
    },
    torso: {
      enabled: true,
      blend: 0.5,
      centerX: 117,
      radiusX: 92,
      radiusZ: 54,
    },
  }
}

function spring(stiffDx: number, softDx: number): Anime25DLayerSpringBinding {
  return {
    stiff: { x: 0, v: 0, dx: stiffDx },
    soft: { x: 0, v: 0, dx: softDx },
    phase: 0,
    stiffnessScale: 1,
    dampingScale: 1,
  }
}

function secondaryFrame(
  progress: number,
  frameIndex: number,
): Anime25DSecondaryDeformationFrame {
  const rotation = Math.sin(progress * 4.7) * 0.16
  return {
    expression: {
      ...IDENTITY_DRIVER,
      angleX: Math.sin(progress * 5.1) * 0.85,
      armPos: Math.cos(progress * 4.3) * 0.72,
      armY: Math.sin(progress * 6.2) * 0.66,
      bangL: Math.sin(progress * 3.2) * 0.7,
      bangC: Math.cos(progress * 4.4) * 0.55,
      bangR: Math.sin(progress * 5.6) * -0.64,
      fhAmp: 0.35 + progress * 1.4,
      fhSoft: 0.2 + progress * 0.7,
      phys: frameIndex % 13 !== 0,
      physAmp: 0.45 + progress * 1.7,
      soft: 0.4 + progress * 1.3,
    },
    faceScale: 0.82,
    headAngleY: Math.cos(progress * 5.4) * 0.9,
    headRotationCosine: Math.cos(rotation),
    headRotationSine: Math.sin(rotation),
    neckPivotX: 117,
    neckPivotY: 161,
    neckBottom: 221,
    neckFollowTop: 148,
    neckFollowSpan: 73,
    faceCenterY: 109,
    bodyBreathOffset: 0.4 + Math.sin(progress * 6.8) * 0.9,
    headBreathOffset: 0.5 + Math.cos(progress * 6.1) * 0.75,
    specialHeadOffset: Math.sin(progress * 7.3) * 5.2,
    highCollar: frameIndex % 2 === 0,
    breath: 0.5 + Math.sin(progress * 5.7) * 0.5,
    chestCenterX: 116,
    chestRegionCenterY: 205,
    chestMotionCenterY: 207 + Math.cos(progress * 3.8) * 13,
    chestRadiusY: 62,
    inverseChestRadiusX: 1 / 78,
    inverseChestRadiusY: 1 / 62,
    chestOffsetX: frameIndex % 11 === 0 ? 0 : Math.sin(progress * 7.2) * 5,
    chestOffsetY: frameIndex % 11 === 0 ? 0 : Math.cos(progress * 6.4) * 7,
    chestField: resolveChestSpatialField({
      source: frameIndex % 3 === 0 ? 'ai-vision' : 'geometry-fallback',
      supportScale: 0.35,
      garmentMotionScale: 0.8,
    }),
    chestVolumeScale: 1,
    shellProfile: shellProfile(),
    shellBlend: 0,
    shellActivation: 0,
    shellRotation: {
      active: false,
      yawCosine: 1,
      yawSine: 0,
      pitchCosine: 1,
      pitchSine: 0,
    },
    torsoProfile: shellProfile().torso,
    torsoChestShape: null,
    torsoShellBlend: 0,
    torsoShellRotation: { active: false, yawCosine: 1, yawSine: 0 },
  }
}

function legacyDeformSecondaryPoint(
  point: { x: number; y: number },
  restX: number,
  restY: number,
  vertex: number,
  binding: Anime25DSecondaryDeformationBinding,
  frame: Anime25DSecondaryDeformationFrame,
): void {
  const { source } = binding
  const baseRole = binding.baseRole
  const isTopwear = baseRole === 'topwear'
  const isFrontCollar = baseRole === 'collar_front'
  const isHead = source.group === 'head'
  if (!binding.shaderGlobalTransform) {
    const neckFollowProgress =
      baseRole === 'neck'
        ? clamp((frame.neckBottom - restY) / frame.neckFollowSpan, 0, 1)
        : 0
    const neckFollowInput = frame.highCollar
      ? neckFollowProgress ** HIGH_COLLAR_NECK_FOLLOW_POWER
      : neckFollowProgress
    const neckHeadBlend = baseRole === 'neck' ? smoothstep(neckFollowInput) : 0
    const frontCollarProgress =
      isFrontCollar && !binding.collarContact
        ? clamp(
            1 -
              (restY - source.y) /
                Math.max(1, source.h * FRONT_COLLAR_FLEX_REGION),
            0,
            1,
          )
        : 0
    const frontCollarLocalX =
      isFrontCollar && !binding.collarContact
        ? Math.abs(
            (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2),
          )
        : 1
    const frontCollarInnerWeight =
      isFrontCollar && !binding.collarContact
        ? smoothstep((1 - frontCollarLocalX) / FRONT_COLLAR_INNER_REGION)
        : 0
    const frontCollarHeadBlend =
      smoothstep(frontCollarProgress) *
      frontCollarInnerWeight *
      FRONT_COLLAR_HEAD_FOLLOW
    let headFollow = isHead ? 1 : source.group === 'body' ? BODY_HEAD_FOLLOW : 0
    if (baseRole === 'neck') {
      headFollow = BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * neckHeadBlend
    } else if (isFrontCollar && !binding.collarContact) {
      headFollow =
        BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * frontCollarHeadBlend
    }
    if (!binding.collarContact && headFollow > 0) {
      const rotationX = point.x - frame.neckPivotX
      const rotationY = point.y - frame.neckPivotY
      const rotatedX =
        rotationX * frame.headRotationCosine -
        rotationY * frame.headRotationSine
      const rotatedY =
        rotationX * frame.headRotationSine +
        rotationY * frame.headRotationCosine
      point.x += (rotatedX - rotationX) * headFollow
      point.y += (rotatedY - rotationY) * headFollow
      let depthOffset =
        (source.depth - 1) * (binding.frontHairParallaxScale?.[vertex] ?? 1)
      if (baseRole === 'neck') depthOffset *= 1 - neckHeadBlend
      else if (isFrontCollar) depthOffset *= 1 - frontCollarHeadBlend
      point.x +=
        headFollow *
        frame.faceScale *
        (frame.expression.angleX * (14 + 40 * depthOffset) +
          frame.expression.angleX * (frame.neckPivotY - point.y) * 0.028)
      point.y +=
        headFollow *
        frame.faceScale *
        (-frame.headAngleY * (9 + 30 * depthOffset) -
          frame.headAngleY * depthOffset * (point.y - frame.faceCenterY) * 0.05)
    }
    if (!binding.collarContact && frame.specialHeadOffset !== 0) {
      const specialHeadFollow = isHead
        ? 1
        : baseRole === 'neck'
          ? neckHeadBlend
          : isFrontCollar
            ? frontCollarHeadBlend
            : 0
      point.y += frame.specialHeadOffset * specialHeadFollow
    }
    if (!binding.collarContact) {
      const breathOffset = isHead
        ? frame.headBreathOffset
        : baseRole === 'neck'
          ? frame.bodyBreathOffset +
            (frame.headBreathOffset - frame.bodyBreathOffset) * neckHeadBlend
          : isFrontCollar
            ? frame.bodyBreathOffset +
              (frame.headBreathOffset - frame.bodyBreathOffset) *
                frontCollarHeadBlend
            : frame.bodyBreathOffset
      point.y -= breathOffset * frame.faceScale
    }
  }
  if (isTopwear && point.y < frame.chestRegionCenterY) {
    point.y -=
      frame.breath *
      2.2 *
      frame.faceScale *
      smoothstep(
        (frame.chestRegionCenterY - point.y) / (frame.chestRadiusY * 2),
      )
  }
  if (isTopwear) {
    point.x =
      frame.neckPivotX +
      (point.x - frame.neckPivotX) * (1 + frame.breath * 0.003)
  }
  if (isTopwear && (frame.chestOffsetX !== 0 || frame.chestOffsetY !== 0)) {
    const normalizedX = (restX - frame.chestCenterX) * frame.inverseChestRadiusX
    const normalizedY =
      (restY - frame.chestMotionCenterY) * frame.inverseChestRadiusY
    const skinWeight = binding.chestWeights?.[vertex] ?? 1
    const chestWeight = chestDeformationWeight(
      frame.chestField,
      normalizedX,
      normalizedY,
      skinWeight,
    )
    point.x += frame.chestOffsetX * chestWeight
    point.y += frame.chestOffsetY * chestWeight
  }
  if (baseRole === 'handwear') {
    const sleeveWeight = smoothstep(((point.y - source.y) / source.h) * 1.15)
    point.y -= frame.expression.armY * 30 * frame.faceScale * sleeveWeight
    point.y += frame.expression.armPos * 40 * frame.faceScale
    point.x +=
      frame.expression.armY *
      6 *
      frame.faceScale *
      sleeveWeight *
      (point.x < frame.neckPivotX ? 1 : -1)
  }
}

function legacyDeformHairPoint(
  point: { x: number; y: number },
  vertex: number,
  binding: Anime25DSecondaryDeformationBinding,
  frame: Anime25DSecondaryDeformationFrame,
): void {
  if (binding.bangWeights && binding.alongStrand) {
    const along = binding.alongStrand[vertex]
    const amplitude = along ** 1.4 * 22 * frame.faceScale
    point.x +=
      (frame.expression.bangL * binding.bangWeights[vertex * 3] +
        frame.expression.bangC * binding.bangWeights[vertex * 3 + 1] +
        frame.expression.bangR * binding.bangWeights[vertex * 3 + 2]) *
      amplitude
  }
  const strandCount = binding.springs?.length ?? 0
  if (
    !strandCount ||
    !binding.springs ||
    !binding.strandWeights ||
    !binding.alongStrand ||
    !frame.expression.phys
  ) {
    return
  }
  const along = binding.alongStrand[vertex]
  const easedAlong = binding.frontHair ? Math.min(1, along * 1.6) : along
  const amplitude =
    easedAlong ** (binding.frontHair ? 1.8 : 2.1) *
    (binding.frontHair ? frame.expression.fhAmp : frame.expression.physAmp)
  const softMix =
    easedAlong ** 1.2 *
    (binding.frontHair ? frame.expression.fhSoft : frame.expression.soft)
  let offsetX = 0
  for (let strand = 0; strand < strandCount; strand += 1) {
    const weight = binding.strandWeights[vertex * strandCount + strand]
    if (weight < 0.001) continue
    const spring = binding.springs[strand]
    offsetX +=
      weight * (spring.stiff.dx * (1 - softMix) + spring.soft.dx * softMix)
  }
  const offset = offsetX * amplitude
  point.x += offset
  point.y += Math.abs(offset) * 0.12
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
