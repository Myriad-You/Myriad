import type { ArmRig, ArmRigMesh } from './armRig'
import type { ChestSpatialField } from './chestPhysics'
import type { Anime25DDriver } from './driver'
import type { HeadTurn, HeadTurnFeature } from './headTurn'
import type { Anime25DLayerSpringBinding } from './layerBinding'
import type { BoundPoseCorrection } from './poseCorrections'
import type {
  Anime25DShellMode,
  Anime25DShellRotation,
} from './shellDeformation'
import type { Anime25DStandingFrame, StandingBinding } from './standing'
import type {
  Anime25DTorsoChestShape,
  Anime25DTorsoShellMode,
  Anime25DTorsoShellRotation,
} from './torsoDeformation'
import type { BoundTurnKeyform } from './turnKeyforms'
import type {
  Anime25DPlaybackLayer,
  Anime25DShellProfile,
  Anime25DTorsoShellProfile,
} from './types'
import { anime25DLayerUsesFaceSurface } from '../rig/anime25dLayerSemantics'
import { ARM_CUT_BAND, ARM_SHRUG, armDrapeWeight, armSegmentShares } from './armRig'
import { chestDeformationWeight } from './chestPhysics'
import {
  BODY_HEAD_FOLLOW,
  collarNeckHeadBlend,
  FRONT_COLLAR_FLEX_REGION,
  FRONT_COLLAR_HEAD_FOLLOW,
  FRONT_COLLAR_INNER_REGION,
} from './collarRuntime'
import { DEFAULT_FRONT_HAIR_SWAY, DEFAULT_REAR_HAIR_SWAY } from './driver'
import { hairChainOffset, hairChainTurn } from './hairChain'
import { headTurnNeckOffset } from './headTurn'
import { applyPoseCorrections } from './poseCorrections'
import { bodyLeanShare } from './poseScale'
import { deformAnime25DShellPoint, shellTurnBlend } from './shellDeformation'
import { applyStanding } from './standing'
import {
  anime25DSleeveAnchorX,
  anime25DTorsoShellOffsetX,
  deformAnime25DTorsoShellPoint,
  SLEEVE_TORSO_TRANSMISSION,
} from './torsoDeformation'
import { addKeyedTurn, unkeyedTurn } from './turnKeyforms'

type SecondaryDeformationDriver = Pick<
  Anime25DDriver,
  | 'angleX'
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

/** The share of a head tilt that the lowest hair still takes. */
const HAIR_DRAPE_ROLL_FLOOR = 0.2

export interface Anime25DSecondaryDeformationFrame {
  expression: Readonly<SecondaryDeformationDriver>
  faceScale: number
  headAngleY: number
  headRotationCosine: number
  headRotationSine: number
  /** The head roll itself, in radians; hair below the shoulders takes less of it. */
  headRoll?: number
  /** Below the neck, hair turns from following the head to resting on the body over this length. */
  hairDrapeLength?: number
  /** The canvas cut the torso bends from; a head tilt carried by the body fades out onto it. */
  bodyPivotY?: number
  bodyBendHeight?: number
  bodyRotationCosine: number
  bodyRotationSine: number
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
  /** Image-plane shoulder rotation of the L and R sleeve drawings. */
  armAngleL: number
  armAngleR: number
  /** The same rotation for cloth hanging from each arm, which lags and sags. */
  armDrapeL: number
  armDrapeR: number
  /** Each forearm's turn about its elbow, relative to its upper arm. */
  forearmL: number
  forearmR: number
  /** Each hand's turn about its wrist, relative to its forearm. */
  handL: number
  handR: number
  /**
   * How far each forearm is raised towards the viewer, 0 hanging to 1 fully
   * reached: drawn shorter, with its hand nearer and larger.
   */
  reachL: number
  reachR: number
  chestCenterX: number
  chestRegionCenterY: number
  chestMotionCenterY: number
  chestRadiusY: number
  inverseChestRadiusX: number
  inverseChestRadiusY: number
  chestOffsetX: number
  chestOffsetY: number
  /** The chest's own stretch (+) or squash (−) as its spring carries it; it keeps its area. */
  chestStretch?: number
  chestField: Readonly<ChestSpatialField>
  chestVolumeScale: number
  shellProfile: Readonly<Anime25DShellProfile>
  shellBlend: number
  shellActivation: number
  shellRotation: Readonly<Anime25DShellRotation>
  /** Turns and nods the head on its drawn outline; without one the ellipsoid does. */
  headTurn?: Readonly<HeadTurn>
  torsoProfile: Readonly<Anime25DTorsoShellProfile>
  torsoChestShape: Anime25DTorsoChestShape | null
  torsoShellBlend: number
  torsoNeckOffsetX: number
  torsoShellRotation: Readonly<Anime25DTorsoShellRotation>
  /** A standing figure's stance; a bust has none. */
  standing?: Readonly<Anime25DStandingFrame> | null
}

export interface Anime25DSecondaryDeformationBinding {
  facialSurface: boolean
  poseCorrections?: BoundPoseCorrection[]
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
  turnFeature?: HeadTurnFeature | null
  turnKeyform?: BoundTurnKeyform | null
  arm: ArmRig | null
  armMesh: ArmRigMesh | null
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
  /** How a standing figure's skirt or leg follows its stance. */
  standing: StandingBinding | null
}

export interface Anime25DMutableSecondaryPoint {
  x: number
  y: number
}

export function createAnime25DSecondaryDeformationBinding(input: {
  poseCorrections?: BoundPoseCorrection[]
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
  arm?: ArmRig | null
  armMesh?: ArmRigMesh | null
  /** A shared torso carry point for arms that move as one piece. */
  handwearAnchorX?: number
  /** The drawn feature this layer belongs to, and its part's keyed turn per vertex. */
  turnFeature?: HeadTurnFeature | null
  turnKeyform?: BoundTurnKeyform | null
  standing?: StandingBinding | null
}): Anime25DSecondaryDeformationBinding {
  return {
    ...input,
    head: input.source.group === 'head',
    facialSurface: anime25DLayerUsesFaceSurface(input.source),
    topwear: input.baseRole === 'topwear',
    frontCollar: input.baseRole === 'collar_front',
    rearCollar: input.baseRole === 'collar_back',
    handwear: input.baseRole === 'handwear',
    handwearSide:
      input.baseRole === 'handwear' ? (input.source.side ?? null) : null,
    handwearAnchorX: input.handwearAnchorX ?? input.source.x + input.source.w / 2,
    shellMode: input.shellMode ?? null,
    hairlinePinWeights: input.hairlinePinWeights ?? null,
    torsoShellMode: input.torsoShellMode ?? null,
    standing: input.standing ?? null,
    arm: input.baseRole === 'handwear' ? (input.arm ?? null) : null,
    armMesh: input.baseRole === 'handwear' ? (input.armMesh ?? null) : null,
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
  let torsoNeckFollow = 0
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
    torsoNeckFollow = binding.head ? 1
      : verticalNeckFollow ? neckHeadBlend
        : binding.frontCollar && !binding.collarContact ? frontCollarHeadBlend : 0
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
      // The shell already carries the nose/eye/mouth relief. Per-layer drawing
      // depths would separate lashes, whites and pupils a second time in yaw.
      // Fade into the shared surface with the existing package activation.
      const surfaceDepth = binding.facialSurface && binding.shellMode === 'head' &&
        frame.shellProfile.enabled && frame.shellProfile.blend > 0 && frame.shellProfile.faceProfile.enabled
        ? source.depth + (1 - source.depth) * frame.shellActivation
        : source.depth
      const localX = point.x
      const localY = point.y
      let rollCosine = frame.headRotationCosine
      let rollSine = frame.headRotationSine
      if (frame.headRoll && restY > frame.neckBottom) {
        // Anything the canvas cuts stays on the cut, so a tilt carried below
        // the neck fades out toward it the way the torso's own lean does.
        let share = frame.bodyBendHeight
          ? bodyLeanShare(restY, frame.bodyPivotY ?? restY, frame.bodyBendHeight)
          : 1
        if (binding.head && frame.hairDrapeLength) {
          // Long hair hangs over the shoulders: it swings with a tilted head
          // near the neck, but its lower length rests on the body.
          const drape = smoothstep((restY - frame.neckBottom) / frame.hairDrapeLength)
          share *= 1 - (1 - HAIR_DRAPE_ROLL_FLOOR) * drape
        }
        if (share < 1) {
          const roll = frame.headRoll * share
          rollCosine = Math.cos(roll)
          rollSine = Math.sin(roll)
        }
      }
      const rotationX = point.x - frame.neckPivotX
      const rotationY = point.y - frame.neckPivotY
      const rotatedX =
        rotationX * rollCosine -
        rotationY * rollSine
      const rotatedY =
        rotationX * rollSine +
        rotationY * rollCosine
      point.x += (rotatedX - rotationX) * headFollow
      point.y += (rotatedY - rotationY) * headFollow
      const rolledX = point.x
      let depthOffset =
        (surfaceDepth - 1) *
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
        // Yaw/pitch deform the head in its own coordinates; roll is its parent transform.
        point.x = localX
        point.y = localY
        deformAnime25DShellPoint(
          point,
          restY,
          binding.shellMode,
          frame.shellProfile,
          frame.shellRotation,
          surfaceDepth,
          binding.turnKeyform ? unkeyedTurn(frame.headTurn, binding.turnKeyform) : frame.headTurn,
          binding.turnFeature,
          binding.baseRole === 'ears' || binding.baseRole === 'earwear',
        )
        if (binding.turnKeyform && frame.headTurn) addKeyedTurn(point, binding.turnKeyform, vertex, frame.headTurn.amount, frame.headTurn.nodAmount)
        if (binding.poseCorrections) applyPoseCorrections(point, vertex, binding.poseCorrections)
        const shellX = point.x - frame.neckPivotX
        const shellY = point.y - frame.neckPivotY
        point.x += (shellX * rollCosine - shellY * rollSine - shellX) * headFollow
        point.y += (shellX * rollSine + shellY * rollCosine - shellY) * headFollow
        const turnBlend = shellTurnBlend(frame)
        point.x = legacyX + (point.x - legacyX) * turnBlend
        point.y = legacyY + (point.y - legacyY) * turnBlend
      } else if (binding.turnKeyform && frame.headTurn) {
        // A keyed neck takes the shape measured on the turned pictures, across
        // and up and down, instead of the computed carry; the roll above and
        // the body below still carry it.
        addKeyedTurn(point, binding.turnKeyform, vertex, frame.headTurn.amount, frame.headTurn.nodAmount)
      } else {
        point.x = legacyX
        point.y = legacyY
        // The top of the neck twists with the turning head instead of the flat carry.
        if (verticalNeckFollow && neckHeadBlend > 0 && frame.headTurn?.active)
          point.x += (headTurnNeckOffset(frame.headTurn, localX, restY) - (legacyX - rolledX)) * neckHeadBlend
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
    if (frame.chestStretch) {
      const scale = 1 + frame.chestStretch * chestWeight
      point.x += (restX - frame.chestCenterX) * (1 / scale - 1)
      point.y += (restY - frame.chestMotionCenterY) * (scale - 1)
    }
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
    // Lifting rolls the shoulder up a little; the rest of the lift is rotation.
    const shrug = frame.expression.armY * ARM_SHRUG * frame.faceScale
    point.y -= shrug
    const arm = binding.arm
    const left = binding.handwearSide === 'L'
    const swing = arm ? (left ? frame.armAngleL : frame.armAngleR) * arm.scale : 0
    const drape = arm?.drape ? (left ? frame.armDrapeL : frame.armDrapeR) * arm.scale : swing
    const angle = arm
      ? swing * (binding.armMesh?.weights[vertex] ?? 1) +
        (drape - swing) * armDrapeWeight(arm, restY)
      : 0
    // Below a found wrist the hand first turns about it, below a found elbow
    // the forearm then turns about that, carrying the wrist; the whole arm
    // then turns about the shoulder, carrying the elbow with it.
    let armX = restX
    let armY = restY
    const shares = arm?.elbow ? armShares(arm, binding.armMesh, vertex, restX, restY) : null
    const foreShare = shares?.fore ?? 0
    const handShare = shares?.hand ?? 0
    // A forearm raised towards the viewer is seen end-on: shorter, its hand
    // brought up the arm and nearer, so larger. The wrist moves with it.
    let wristX = arm?.wrist?.x ?? 0
    let wristY = arm?.wrist?.y ?? 0
    const reach = arm?.wrist ? (left ? frame.reachL : frame.reachR) * arm.scale : 0
    if (arm?.elbow && arm.wrist && reach > 0) {
      const ux = arm.wrist.x - arm.elbow.x
      const uy = arm.wrist.y - arm.elbow.y
      const length = Math.hypot(ux, uy)
      if (length > 0) {
        const shorten = REACH_SHORTEN * reach
        const grow = REACH_HAND_GROW * reach
        const ax = ux / length
        const ay = uy / length
        const along = (restX - arm.elbow.x) * ax + (restY - arm.elbow.y) * ay
        const across = (restX - arm.elbow.x) * -ay + (restY - arm.elbow.y) * ax
        const wristAlong = length * (1 - shorten)
        const inHand = handShare
        let seen = along <= length ? along * (1 - shorten) : along - shorten * length
        seen += (seen - wristAlong) * grow * inHand
        seen = along + (seen - along) * foreShare
        const width = across * (1 + grow * inHand)
        armX = arm.elbow.x + ax * seen - ay * width
        armY = arm.elbow.y + ay * seen + ax * width
        wristX = arm.elbow.x + ax * wristAlong
        wristY = arm.elbow.y + ay * wristAlong
      }
    }
    const hand = arm?.wrist ? (left ? frame.handL : frame.handR) * arm.scale : 0
    if (arm?.wrist && hand !== 0) {
      const share = hand * handShare
      if (share !== 0) {
        const wx = armX - wristX
        const wy = armY - wristY
        const cosine = Math.cos(share)
        const sine = Math.sin(share)
        armX = wristX + wx * cosine - wy * sine
        armY = wristY + wx * sine + wy * cosine
      }
    }
    const forearm = arm?.elbow ? (left ? frame.forearmL : frame.forearmR) * arm.scale : 0
    if (arm?.elbow && forearm !== 0) {
      const share = forearm * foreShare
      if (share !== 0) {
        const ex = armX - arm.elbow.x
        const ey = armY - arm.elbow.y
        const cosine = Math.cos(share)
        const sine = Math.sin(share)
        armX = arm.elbow.x + ex * cosine - ey * sine
        armY = arm.elbow.y + ex * sine + ey * cosine
      }
    }
    point.x += armX - restX
    point.y += armY - restY
    if (arm && angle !== 0) {
      // About the shoulder joint, in rest space. Everything applied above is a
      // uniform carry of the whole sleeve, so the joint travels with it.
      const dx = armX - arm.pivotX
      const dy = armY - arm.pivotY
      const cosine = Math.cos(angle)
      const sine = Math.sin(angle)
      point.x += dx * cosine - dy * sine - dx
      point.y += dx * sine + dy * cosine - dy
    }
    if (arm?.cutY != null) {
      // The crop hides the rest of the arm. Its cut edge slides along the frame
      // line instead of lifting off it or following the shrug: the lower band
      // takes up the difference this column's cut point would otherwise show.
      const band = (arm.cutY - arm.pivotY) * ARM_CUT_BAND
      const blend = smoothstep((restY - arm.cutY + band) / band)
      if (blend > 0) {
        const cutDy = arm.cutY - arm.pivotY
        const cutAngle = swing + (drape - swing) * armDrapeWeight(arm, arm.cutY)
        const lifted =
          cutDy - ((restX - arm.pivotX) * Math.sin(cutAngle) + cutDy * Math.cos(cutAngle))
        point.y += (lifted + shrug) * blend
      }
    }
  }
  // Torso translation is the parent of the head's local yaw/pitch/roll.
  // The neck's lower part already receives its cylinder projection above;
  // only its head-follow share inherits the root, avoiding double travel.
  point.x += frame.torsoNeckOffsetX * torsoNeckFollow
  if (binding.standing && frame.standing) applyStanding(point, binding.standing, restX, restY, frame.standing)
}

/** A fully reached forearm is drawn this much shorter, and its hand this much larger. */
const REACH_SHORTEN = 0.35
const REACH_HAND_GROW = 0.15

/** The shares bound with the mesh, or worked out for a point bound without one. */
const segmentShares = { fore: 0, hand: 0 }
function armShares(arm: Readonly<ArmRig>, mesh: ArmRigMesh | null, vertex: number, x: number, y: number) {
  if (mesh && vertex >= 0 && vertex < mesh.fore.length) {
    segmentShares.fore = mesh.fore[vertex]
    segmentShares.hand = mesh.hand[vertex]
  } else {
    armSegmentShares(arm, x, y, segmentShares)
  }
  return segmentShares
}

const chainPoint = { x: 0, y: 0 }
/** Half a lock's width, as a share of its length, for turning its width with its bend. */
const LOCK_HALF_WIDTH_SHARE = 0.15

export function deformAnime25DHairPoint(
  point: Anime25DMutableSecondaryPoint,
  vertex: number,
  binding: Readonly<Anime25DSecondaryDeformationBinding>,
  frame: Readonly<Anime25DSecondaryDeformationFrame>,
  /** Rest x of the vertex; with it, the lock's width turns with its bend. */
  restX?: number,
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
  // The chain carries the shape of the swing; the drivers only scale it.
  const amplitude = binding.frontHair
    ? frame.expression.fhAmp / DEFAULT_FRONT_HAIR_SWAY
    : frame.expression.physAmp / DEFAULT_REAR_HAIR_SWAY
  let offsetX = 0
  let offsetY = 0
  for (let strand = 0; strand < springs.length; strand += 1) {
    // Binding weights also carry the old length amplitude; the chain's own length already does that.
    const weight = strandWeights[vertex * springs.length + strand] / springs[strand].amplitudeScale
    if (weight < 0.001) continue
    const chain = springs[strand].chain
    hairChainOffset(chain, along, chainPoint)
    offsetX += weight * chainPoint.x
    offsetY += weight * chainPoint.y
    if (restX !== undefined) {
      // Across the lock, the drawing turns with it like a ribbon instead of
      // shearing. A lock is far narrower than it is long; hair further out
      // belongs to its neighbours, not to a wide plank swinging round this one.
      const reach = LOCK_HALF_WIDTH_SHARE * chain.linkLength * chain.links
      const across = clamp(restX - springs[strand].rootX, -reach, reach)
      const turn = hairChainTurn(chain, along)
      offsetX += weight * across * (Math.cos(turn) - 1)
      offsetY += weight * across * Math.sin(turn)
    }
  }
  const scale = amplitude * motionScale
  // Inputs are sampled after the body transform. Bring the lag vector back
  // into this CPU mesh's coordinates; the shader rotates it exactly once.
  point.x += (offsetX * frame.bodyRotationCosine + offsetY * frame.bodyRotationSine) * scale
  point.y += (-offsetX * frame.bodyRotationSine + offsetY * frame.bodyRotationCosine) * scale
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

export { shellTurnBlend } from './shellDeformation'
