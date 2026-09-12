import type { ChestSpatialField } from './chestPhysics'
import type { Anime25DDriver } from './driver'
import type { Anime25DLayerSpringBinding } from './layerBinding'
import type {
  Anime25DShellMode,
  Anime25DShellRotation,
} from './shellDeformation'
import type {
  Anime25DTorsoChestShape,
  Anime25DTorsoShellMode,
  Anime25DTorsoShellRotation,
} from './torsoDeformation'
import type {
  Anime25DPlaybackLayer,
  Anime25DShellProfile,
  Anime25DTorsoShellProfile,
} from './types'
import { ARM_SWING_SPREAD, ARM_SWING_TRAVEL } from './armFollow'
import { chestDeformationWeight } from './chestPhysics'
import {
  BODY_HEAD_FOLLOW,
  collarNeckHeadBlend,
  FRONT_COLLAR_FLEX_REGION,
  FRONT_COLLAR_HEAD_FOLLOW,
  FRONT_COLLAR_INNER_REGION,
} from './collarRuntime'
import { deformAnime25DShellPoint } from './shellDeformation'
import {
  anime25DSleeveAnchorX,
  anime25DTorsoShellOffsetX,
  deformAnime25DTorsoShellPoint,
  SLEEVE_TORSO_TRANSMISSION,
} from './torsoDeformation'

type SecondaryDeformationDriver = Pick<
  Anime25DDriver,
  | 'angleX'
  | 'armPos'
  | 'armY'
  | 'bangC'
  | 'bangL'
  | 'bangR'
  | 'fhAmp'
  | 'fhSoft'
  | 'phys'
  | 'physAmp'
  | 'soft'
>

export interface Anime25DSecondaryDeformationFrame {
  expression: Readonly<SecondaryDeformationDriver>
  faceScale: number
  headAngleY: number
  headRotationCosine: number
  headRotationSine: number
  neckPivotX: number
  neckPivotY: number
  neckBottom: number
  neckFollowTop: number
  neckFollowSpan: number
  faceCenterY: number
  bodyBreathOffset: number
  headBreathOffset: number
  specialHeadOffset: number
  highCollar: boolean
  breath: number
  armSwing: number
  chestCenterX: number
  chestRegionCenterY: number
  chestMotionCenterY: number
  chestRadiusY: number
  inverseChestRadiusX: number
  inverseChestRadiusY: number
  chestOffsetX: number
  chestOffsetY: number
  chestField: Readonly<ChestSpatialField>
  chestVolumeScale: number
  shellProfile: Readonly<Anime25DShellProfile>
  shellBlend: number
  shellActivation: number
  shellRotation: Readonly<Anime25DShellRotation>
  torsoProfile: Readonly<Anime25DTorsoShellProfile>
  torsoChestShape: Anime25DTorsoChestShape | null
  torsoShellBlend: number
  torsoShellRotation: Readonly<Anime25DTorsoShellRotation>
}

export interface Anime25DSecondaryDeformationBinding {
  source: Pick<
    Anime25DPlaybackLayer,
    'depth' | 'group' | 'h' | 'role' | 'side' | 'w' | 'x' | 'y'
  >
  baseRole: string
  shaderGlobalTransform: boolean
  collarContact: boolean
  head: boolean
  topwear: boolean
  frontCollar: boolean
  rearCollar: boolean
  handwear: boolean
  handwearSide: Anime25DPlaybackLayer['side']
  handwearAnchorX: number
  frontHair: boolean
  frontHairParallaxScale: Float32Array | null
  chestWeights: Float32Array | null
  bangWeights: Float32Array | null
  strandWeights: Float32Array | null
  alongStrand: Float32Array | null
  springs: Anime25DLayerSpringBinding[] | null
  shellMode: Anime25DShellMode | null
  hairlinePinWeights: Float32Array | null
  torsoShellMode: Anime25DTorsoShellMode | null
}

export interface Anime25DMutableSecondaryPoint {
  x: number
  y: number
}

export function createAnime25DSecondaryDeformationBinding(input: {
  source: Anime25DSecondaryDeformationBinding['source']
  baseRole: string
  shaderGlobalTransform: boolean
  collarContact: boolean
  frontHair: boolean
  frontHairParallaxScale: Float32Array | null
  chestWeights: Float32Array | null
  bangWeights: Float32Array | null
  strandWeights: Float32Array | null
  alongStrand: Float32Array | null
  springs: Anime25DLayerSpringBinding[] | null
  shellMode?: Anime25DShellMode | null
  hairlinePinWeights?: Float32Array | null
  torsoShellMode?: Anime25DTorsoShellMode | null
}): Anime25DSecondaryDeformationBinding {
  return {
    ...input,
    head: input.source.group === 'head',
    topwear: input.baseRole === 'topwear',
    frontCollar: input.baseRole === 'collar_front',
    rearCollar: input.baseRole === 'collar_back',
    handwear: input.baseRole === 'handwear',
    handwearSide:
      input.baseRole === 'handwear' ? (input.source.side ?? null) : null,
    handwearAnchorX: input.source.x + input.source.w / 2,
    shellMode: input.shellMode ?? null,
    hairlinePinWeights: input.hairlinePinWeights ?? null,
    torsoShellMode: input.torsoShellMode ?? null,
  }
}

export function deformAnime25DSecondaryPoint(
  point: Anime25DMutableSecondaryPoint,
  restX: number,
  restY: number,
  vertex: number,
  binding: Readonly<Anime25DSecondaryDeformationBinding>,
  frame: Readonly<Anime25DSecondaryDeformationFrame>,
): void {
  const { source } = binding
  let collarBodyWeight = 1
  if (!binding.shaderGlobalTransform) {
    const contourCollar =
      binding.rearCollar || (binding.frontCollar && binding.collarContact)
    const verticalNeckFollow = binding.baseRole === 'neck' || contourCollar
    const neckFollowProgress = verticalNeckFollow
      ? clamp((frame.neckBottom - restY) / frame.neckFollowSpan, 0, 1)
      : 0
    const neckHeadBlend =
      verticalNeckFollow && frame.highCollar
        ? collarNeckHeadBlend(restY, frame)
        : verticalNeckFollow
          ? smoothstep(neckFollowProgress)
          : 0
    const frontCollarProgress =
      binding.frontCollar && !binding.collarContact
        ? clamp(
            1 -
              (restY - source.y) /
                Math.max(1, source.h * FRONT_COLLAR_FLEX_REGION),
            0,
            1,
          )
        : 0
    const frontCollarLocalX =
      binding.frontCollar && !binding.collarContact
        ? Math.abs(
            (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2),
          )
        : 1
    const frontCollarInnerWeight =
      binding.frontCollar && !binding.collarContact
        ? smoothstep((1 - frontCollarLocalX) / FRONT_COLLAR_INNER_REGION)
        : 0
    const frontCollarNeckWeight =
      smoothstep(frontCollarProgress) * frontCollarInnerWeight
    const frontCollarHeadBlend =
      frontCollarNeckWeight * FRONT_COLLAR_HEAD_FOLLOW
    if (verticalNeckFollow) {
      collarBodyWeight = 1 - neckHeadBlend
    } else if (binding.frontCollar) {
      collarBodyWeight = 1 - frontCollarNeckWeight
    }
    let headFollow = binding.head
      ? 1
      : source.group === 'body'
        ? BODY_HEAD_FOLLOW
        : 0
    if (verticalNeckFollow) {
      headFollow = BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * neckHeadBlend
    } else if (binding.frontCollar && !binding.collarContact) {
      headFollow =
        BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * frontCollarHeadBlend
    }
    if (headFollow > 0) {
      const localX = point.x
      const localY = point.y
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
        (source.depth - 1) *
        (binding.frontHairParallaxScale?.[vertex] ?? 1)
      if (verticalNeckFollow) depthOffset *= 1 - neckHeadBlend
      else if (binding.frontCollar) depthOffset *= 1 - frontCollarHeadBlend
      const legacyX =
        point.x +
        headFollow *
          frame.faceScale *
          (frame.expression.angleX * (14 + 40 * depthOffset) +
            frame.expression.angleX * (frame.neckPivotY - point.y) * 0.028)
      const legacyY =
        point.y +
        headFollow *
          frame.faceScale *
          (-frame.headAngleY * (9 + 30 * depthOffset) -
            frame.headAngleY *
              depthOffset *
              (point.y - frame.faceCenterY) *
              0.05)
      if (binding.shellMode && frame.shellProfile.enabled) {
        // Yaw/pitch deform the head in its own coordinates. Roll is its parent
        // transform; sampling an unrotated shell with rolled points changes shape.
        point.x = localX
        point.y = localY
        deformAnime25DShellPoint(
          point,
          restY,
          binding.shellMode,
          frame.shellProfile,
          frame.shellRotation,
          source.depth,
        )
        const shellX = point.x - frame.neckPivotX
        const shellY = point.y - frame.neckPivotY
        point.x += (shellX * frame.headRotationCosine - shellY * frame.headRotationSine - shellX) * headFollow
        point.y += (shellX * frame.headRotationSine + shellY * frame.headRotationCosine - shellY) * headFollow
        point.x = legacyX + (point.x - legacyX) * frame.shellBlend
        point.y = legacyY + (point.y - legacyY) * frame.shellBlend
      } else {
        point.x = legacyX
        point.y = legacyY
      }
    }
    if (!binding.collarContact && frame.specialHeadOffset !== 0) {
      const specialHeadFollow = binding.head
        ? 1
        : binding.baseRole === 'neck'
          ? neckHeadBlend
          : binding.frontCollar
            ? frontCollarHeadBlend
            : 0
      point.y += frame.specialHeadOffset * specialHeadFollow
    }
    if (!binding.collarContact || contourCollar) {
      const breathOffset = binding.head
        ? frame.headBreathOffset
        : verticalNeckFollow
          ? frame.bodyBreathOffset +
            (frame.headBreathOffset - frame.bodyBreathOffset) * neckHeadBlend
          : binding.frontCollar
            ? frame.bodyBreathOffset +
              (frame.headBreathOffset - frame.bodyBreathOffset) *
                frontCollarHeadBlend
            : frame.bodyBreathOffset
      point.y -= breathOffset * frame.faceScale
    }
  }
  if (binding.topwear && point.y < frame.chestRegionCenterY) {
    point.y -=
      frame.breath *
      2.2 *
      frame.faceScale *
      smoothstep(
        (frame.chestRegionCenterY - point.y) / (frame.chestRadiusY * 2),
      )
  }
  if (binding.topwear) {
    point.x =
      frame.neckPivotX +
      (point.x - frame.neckPivotX) * (1 + frame.breath * 0.003)
  }
  if (
    binding.topwear &&
    (frame.chestOffsetX !== 0 || frame.chestOffsetY !== 0)
  ) {
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
  if (binding.torsoShellMode === 'sleeve') {
    // A sleeve is one rigid drawing hanging beside the cylinder, not a patch of its surface.
    point.x += anime25DTorsoShellOffsetX(
      anime25DSleeveAnchorX(binding.handwearAnchorX, frame.torsoProfile),
      frame.torsoProfile,
      frame.torsoShellRotation,
      frame.torsoShellBlend * SLEEVE_TORSO_TRANSMISSION,
    )
  } else if (binding.torsoShellMode) {
    const torsoWeight =
      binding.torsoShellMode === 'collar' || binding.torsoShellMode === 'neck'
        ? collarBodyWeight
        : 1
    deformAnime25DTorsoShellPoint(
      point,
      frame.torsoProfile,
      frame.torsoShellRotation,
      frame.torsoShellBlend * torsoWeight,
      binding.topwear ? frame.torsoChestShape : null,
      binding.topwear ? (binding.chestWeights?.[vertex] ?? 1) : 1,
      frame.chestVolumeScale,
    )
  }
  if (binding.handwear) {
    const sleeveWeight = smoothstep(((point.y - source.y) / source.h) * 1.15)
    const sleeveSide =
      binding.handwearSide === 'L' ? 1 : binding.handwearSide === 'R' ? -1 : 0
    point.y -= frame.expression.armY * 30 * frame.faceScale * sleeveWeight
    point.y += frame.expression.armPos * 40 * frame.faceScale
    point.x +=
      frame.expression.armY * 6 * frame.faceScale * sleeveWeight * sleeveSide
    point.x +=
      frame.armSwing *
      (ARM_SWING_TRAVEL + ARM_SWING_SPREAD * sleeveSide * frame.armSwing) *
      frame.faceScale *
      sleeveWeight
  }
}

export function deformAnime25DHairPoint(
  point: Anime25DMutableSecondaryPoint,
  vertex: number,
  binding: Readonly<Anime25DSecondaryDeformationBinding>,
  frame: Readonly<Anime25DSecondaryDeformationFrame>,
): void {
  const alongStrand = binding.alongStrand
  const authoredPinWeight = binding.hairlinePinWeights?.[vertex] ?? 0
  const shellActivation = frame.shellActivation
  const pinWeight = authoredPinWeight * shellActivation
  const motionScale = 1 - pinWeight
  if (binding.bangWeights && alongStrand) {
    const along = alongStrand[vertex]
    const amplitude = along ** 1.4 * 22 * frame.faceScale
    point.x +=
      (frame.expression.bangL * binding.bangWeights[vertex * 3] +
        frame.expression.bangC * binding.bangWeights[vertex * 3 + 1] +
        frame.expression.bangR * binding.bangWeights[vertex * 3 + 2]) *
      amplitude *
      motionScale
  }
  const springs = binding.springs
  const strandWeights = binding.strandWeights
  if (
    !springs?.length ||
    !strandWeights ||
    !alongStrand ||
    !frame.expression.phys
  ) {
    return
  }
  const along = alongStrand[vertex]
  const easedAlong = binding.frontHair ? Math.min(1, along * 1.6) : along
  const amplitude =
    easedAlong ** (binding.frontHair ? 1.8 : 2.1) *
    (binding.frontHair ? frame.expression.fhAmp : frame.expression.physAmp)
  const softMix =
    easedAlong ** 1.2 *
    (binding.frontHair ? frame.expression.fhSoft : frame.expression.soft)
  let offsetX = 0
  for (let strand = 0; strand < springs.length; strand += 1) {
    const weight = strandWeights[vertex * springs.length + strand]
    if (weight < 0.001) continue
    const spring = springs[strand]
    offsetX +=
      weight * (spring.stiff.dx * (1 - softMix) + spring.soft.dx * softMix)
  }
  const offset = offsetX * amplitude * motionScale
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
