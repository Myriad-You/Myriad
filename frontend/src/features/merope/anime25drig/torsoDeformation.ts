import type { ChestSpatialField } from './chestPhysics'
import type {
  Anime25DChestProfile,
  Anime25DPlaybackLayer,
  Anime25DShellProfile,
  Anime25DTorsoShellProfile,
} from './types'
import {
  chestDeformationWeight,
  resolveChestSpatialField,
} from './chestPhysics'

export const HEAD_YAW_SHARE = 0.45
const BODY_YAW_SHARE = 0.1
/**
 * A standing figure turns with its sway: at its rig's full range (`body` = ±1)
 * it is turned about 20 degrees toward the leg it goes over, legs and all,
 * rather than sliding sideways. A look carries its torso round by a share of
 * a bust's follow: its whole cylinder shows the turn, where a bust's shows a
 * quarter of it. Its automatic turn stays within FIGURE_MAX_YAW.
 */
export const FIGURE_BODY_YAW = 0.35
const FIGURE_LOOK_SHARE = 0.2
const FIGURE_MAX_YAW = 0.45
// ~0.4 s to 95% for a held target, with continuous position AND velocity.
const YAW_ANGULAR_FREQUENCY = 12
const MIN_GARMENT_SHAPE_TRANSMISSION = 0.4

export const SLEEVE_TORSO_TRANSMISSION = 0.85

/**
 * A standing figure's trunk, as a drawn turn of the figure shows it: the
 * front of the chest and skirt travels across while the head, the arms and
 * the legs hold near the turn's axis, the way an anime three-quarter view
 * draws the body's middle line over toward its far side. Its cylinder is this
 * deep for its half-width (a bust's guess from the face is shallower); the
 * neck stands this share of the way out to the chest's front; a sleeve clear
 * of the body keeps this share of its anchor's travel. Measured on a drawn
 * 28° turn: chest and skirt +69 px, arms, head and legs +8 to +28.
 */
const FIGURE_TORSO_DEPTH = 0.75
const FIGURE_NECK_DEPTH = 0.25
const FIGURE_SLEEVE_HANG = 0.2
/** A sleeve lies over the body out to this many half-widths, letting go over the last share of it. */
const FIGURE_BODY_EDGE = 1.25
const FIGURE_BODY_EDGE_FADE = 0.3

export const SLEEVE_ANCHOR_LIMIT = 0.8

const CHEST_FIELD_LIMIT = 1.5
const CHEST_FIELD_FEATHER = 0.28

export type Anime25DTorsoShellMode = 'full' | 'collar' | 'sleeve' | 'neck'

export interface Anime25DTorsoYawState {
  value: number
  velocity: number
}

export interface Anime25DTorsoShellRotation {
  active: boolean
  yawCosine: number
  yawSine: number
}

export interface Anime25DTorsoChestShape {
  centerX: number
  centerY: number
  radiusX: number
  radiusY: number
  scale: number
  field: Readonly<ChestSpatialField>
}

export interface Anime25DMutableTorsoPoint {
  x: number
  y: number
}

export function anime25DLayerUsesTorsoShell(
  source: Pick<Anime25DPlaybackLayer, 'group' | 'role'>,
): boolean {
  return anime25DTorsoShellModeForLayer(source) !== null
}

export function anime25DTorsoShellModeForLayer(
  source: Pick<Anime25DPlaybackLayer, 'group' | 'role'>,
): Anime25DTorsoShellMode | null {
  if (source.group !== 'body') return null
  if (source.role === 'neck') return 'neck'
  if (source.role === 'collar-front' || source.role === 'collar-back') {
    return 'collar'
  }
  if (source.role === 'topwear' || source.role === 'bottomwear') {
    return 'full'
  }
  if (source.role === 'handwear') return 'sleeve'
  return null
}

export function anime25DTorsoYawFollow(
  profile: Readonly<Pick<Anime25DTorsoShellProfile, 'yawFollowScale'>>,
  bodyYaw: number,
): number {
  const authored = profile.yawFollowScale
  return (
    clamp(Number.isFinite(authored) ? (authored as number) : 1, 0, 1) *
    clamp(Number.isFinite(bodyYaw) ? bodyYaw : 1, 0, 1)
  )
}

export function stepAnime25DTorsoShellRotation(
  state: Anime25DTorsoYawState,
  angleX: number,
  body: number,
  deltaSeconds: number,
  target: Anime25DTorsoShellRotation,
  yawFollowScale = 1,
  directedTurn = 0,
  directedShare = 0,
  bodyShare = BODY_YAW_SHARE,
): void {
  const follow = Number.isFinite(yawFollowScale)
    ? clamp(yawFollowScale, 0, 1)
    : 1
  const share = clamp(Number.isFinite(directedShare) ? directedShare : 0, 0, 1)
  // Only automatic head/body follow has this spring. The directed contribution
  // already has authority and a continuous response in the composer.
  const figure = bodyShare !== BODY_YAW_SHARE
  const raw = angleX * HEAD_YAW_SHARE * follow * (figure ? FIGURE_LOOK_SHARE : 1) + body * bodyShare
  const yawTarget = figure ? Math.max(-FIGURE_MAX_YAW, Math.min(FIGURE_MAX_YAW, raw)) : raw
  if (Number.isFinite(deltaSeconds) && deltaSeconds > 0 && Number.isFinite(yawTarget)) {
    // Exact critically damped response; see The Orange Duck's Spring-Roll-Call.
    // Carry velocity through a new goal instead of starting another easing.
    const displacement = state.value - yawTarget
    const coefficient = state.velocity + YAW_ANGULAR_FREQUENCY * displacement
    const decay = Math.exp(-YAW_ANGULAR_FREQUENCY * deltaSeconds)
    state.value = yawTarget + (displacement + coefficient * deltaSeconds) * decay
    state.velocity = (state.velocity - YAW_ANGULAR_FREQUENCY * coefficient * deltaSeconds) * decay
  }
  const yaw = state.value * (1 - share)
    + clamp(Number.isFinite(directedTurn) ? directedTurn : 0, -1, 1) * HEAD_YAW_SHARE
  target.active = Math.abs(yaw) > 1e-7
  target.yawCosine = Math.cos(yaw)
  target.yawSine = Math.sin(yaw)
}

/** Adds only rotated-minus-frontal cylinder projection, preserving the rest pose. */
export function deformAnime25DTorsoShellPoint(
  point: Anime25DMutableTorsoPoint,
  profile: Readonly<Anime25DTorsoShellProfile>,
  rotation: Readonly<Anime25DTorsoShellRotation>,
  blend: number,
  chestShape: Readonly<Anime25DTorsoChestShape> | null = null,
  chestSkinWeight = 1,
  chestVolumeScale = 1,
  depthScale = 1,
): void {
  point.x += torsoShellOffsetX(
    point.x,
    point.y,
    profile,
    rotation,
    blend,
    chestShape,
    chestSkinWeight,
    chestVolumeScale,
    depthScale,
  )
}

export function anime25DSleeveAnchorX(
  anchorX: number,
  profile: Readonly<Anime25DTorsoShellProfile>,
): number {
  const radiusX = Math.max(1, profile.radiusX)
  const normalized = (anchorX - profile.centerX) / radiusX
  return (
    profile.centerX +
    clamp(normalized, -SLEEVE_ANCHOR_LIMIT, SLEEVE_ANCHOR_LIMIT) * radiusX
  )
}

/** A standing figure's shell: its trunk FIGURE_TORSO_DEPTH deep, its neck near the axis. */
export function figureShellProfile(profile: Readonly<Anime25DShellProfile>): Anime25DShellProfile {
  const torso = { ...profile.torso, radiusZ: profile.torso.radiusX * FIGURE_TORSO_DEPTH, neckDepth: FIGURE_NECK_DEPTH }
  return { ...profile, torso }
}

/** How far the turn carries the neck, and the head on it, across. */
export function anime25DTorsoNeckOffsetX(
  neckX: number,
  profile: Readonly<Anime25DTorsoShellProfile>,
  rotation: Readonly<Anime25DTorsoShellRotation>,
  blend: number,
): number {
  return torsoShellOffsetX(neckX, 0, profile, rotation, blend, null, 1, 1, profile.neckDepth ?? 1)
}

/**
 * How far out a standing figure's front stands at `restY`, as a share of its
 * trunk's depth: at the neck's foot, only as far as the neck (neckDepth), so
 * the neck, the choker on it and the collar round it turn together; down to
 * the chest's middle it comes out to the whole depth, where the chest turns
 * out the most. 1 for a bust, whose front is all the same depth.
 */
export function figureFrontDepth(
  profile: Readonly<Anime25DTorsoShellProfile>,
  restY: number,
  neckBottom: number,
  chestY: number,
): number {
  const neck = profile.neckDepth
  if (neck === undefined) return 1
  return neck + (1 - neck) * smoothstep((restY - neckBottom) / Math.max(1, chestY - neckBottom))
}

/**
 * Moves a point `share` of the way the torso's cylinder takes it, as a
 * standing figure's (its front standing out by figureFrontDepth); returns
 * the share. For a part turned by its key that gives way to the cylinder
 * where it meets the torso.
 */
export function addTorsoShellShare(
  point: Anime25DMutableTorsoPoint,
  share: number,
  frame: Readonly<{
    torsoProfile: Readonly<Anime25DTorsoShellProfile>
    torsoShellRotation: Readonly<Anime25DTorsoShellRotation>
    torsoShellBlend: number
    chestVolumeScale: number
    neckBottom: number
    chestRegionCenterY: number
  }>,
  restY: number,
): number {
  if (share <= 0) return 0
  const x = point.x
  const y = point.y
  deformAnime25DTorsoShellPoint(point, frame.torsoProfile, frame.torsoShellRotation, frame.torsoShellBlend, null, 1,
    frame.chestVolumeScale, figureFrontDepth(frame.torsoProfile, restY, frame.neckBottom, frame.chestRegionCenterY))
  point.x = x + (point.x - x) * share
  point.y = y + (point.y - y) * share
  return share
}

/**
 * A standing figure's sleeve, horizontally: over the body it lies on the
 * cylinder like the garment under it, so their seam holds; clear of the body
 * it hangs at the side, near the turn's axis, with FIGURE_SLEEVE_HANG of a
 * bust sleeve's rigid travel.
 */
export function figureSleeveOffsetX(
  restX: number,
  anchorX: number,
  profile: Readonly<Anime25DTorsoShellProfile>,
  rotation: Readonly<Anime25DTorsoShellRotation>,
  blend: number,
  depthScale = 1,
): number {
  const out = Math.abs(restX - profile.centerX) / Math.max(1, profile.radiusX)
  const clear = clamp((out - (FIGURE_BODY_EDGE - FIGURE_BODY_EDGE_FADE)) / FIGURE_BODY_EDGE_FADE, 0, 1)
  const onBody = torsoShellOffsetX(restX, 0, profile, rotation, blend, null, 1, 1, depthScale)
  const hanging = torsoShellOffsetX(anime25DSleeveAnchorX(anchorX, profile), 0, profile, rotation,
    blend * SLEEVE_TORSO_TRANSMISSION * FIGURE_SLEEVE_HANG, null)
  return onBody + (hanging - onBody) * clear
}

export function anime25DTorsoShellOffsetX(
  pointX: number,
  profile: Readonly<Anime25DTorsoShellProfile>,
  rotation: Readonly<Anime25DTorsoShellRotation>,
  blend: number,
  depthScale = 1,
): number {
  return torsoShellOffsetX(pointX, 0, profile, rotation, blend, null, 1, 1, depthScale)
}

export function resolveAnime25DTorsoChestShape(
  profile: Readonly<Anime25DChestProfile>,
  field: Readonly<ChestSpatialField> = resolveChestSpatialField(profile),
): Anime25DTorsoChestShape | null {
  if (!profile.enabled) return null
  const visible = smoothstep(clamp(profile.visibleScale, 0, 1))
  const garment =
    MIN_GARMENT_SHAPE_TRANSMISSION +
    (1 - MIN_GARMENT_SHAPE_TRANSMISSION) *
      Math.sqrt(clamp(profile.garmentMotionScale, 0, 1))
  const scale = visible * garment
  if (scale <= 1e-6) return null
  return {
    centerX: profile.centerX,
    centerY: profile.centerY,
    radiusX: Math.max(1, profile.radiusX),
    radiusY: Math.max(1, profile.radiusY),
    scale,
    field,
  }
}

function torsoShellOffsetX(
  pointX: number,
  pointY: number,
  profile: Readonly<Anime25DTorsoShellProfile>,
  rotation: Readonly<Anime25DTorsoShellRotation>,
  blend: number,
  chestShape: Readonly<Anime25DTorsoChestShape> | null,
  chestSkinWeight = 1,
  chestVolumeScale = 1,
  depthScale = 1,
): number {
  if (!profile.enabled || blend <= 0 || !rotation.active) return 0
  const x = pointX - profile.centerX
  const normalizedX = x / profile.radiusX
  let z =
    Math.sqrt(Math.max(0, 1 - Math.min(1, normalizedX * normalizedX))) *
    profile.radiusZ * depthScale
  let silhouetteOffset = 0
  if (chestShape) {
    const normalizedChestX = (pointX - chestShape.centerX) / chestShape.radiusX
    const normalizedChestY = (pointY - chestShape.centerY) / chestShape.radiusY
    const fieldDistance = Math.max(
      Math.abs(normalizedChestX),
      Math.abs(normalizedChestY),
    )
    if (fieldDistance < CHEST_FIELD_LIMIT) {
      const direction = clamp(rotation.yawSine * 4, -1, 1)
      const side = normalizedChestX < 0 ? -1 : 1
      const sideDirection = side * direction
      const fieldFade = smoothstep(
        (CHEST_FIELD_LIMIT - fieldDistance) / CHEST_FIELD_FEATHER,
      )
      const pairedWeight =
        chestDeformationWeight(
          chestShape.field,
          normalizedChestX,
          normalizedChestY,
          chestSkinWeight,
        ) * fieldFade
      const sideGain =
        1 +
        chestShape.field.nearDepthGain * Math.max(0, sideDirection) +
        chestShape.field.farDepthGain * Math.max(0, -sideDirection)
      z +=
        profile.radiusZ *
        chestShape.field.depthRatio *
        chestShape.scale *
        clamp(chestVolumeScale, 0.9, 1.1) *
        pairedWeight *
        sideGain

      const edge =
        smoothstep((Math.abs(normalizedX) - 0.2) / 0.5) *
        smoothstep((1.18 - Math.abs(normalizedX)) / 0.35)
      silhouetteOffset =
        side *
        profile.radiusX *
        chestShape.field.silhouetteRatio *
        chestShape.scale *
        pairedWeight *
        fieldFade *
        Math.max(0, sideDirection) *
        edge
    }
  }
  const rotatedX = x * rotation.yawCosine + z * rotation.yawSine
  const rotatedZ = -x * rotation.yawSine + z * rotation.yawCosine
  const focalLength = Math.max(1, profile.radiusZ * 6)
  const rotatedScale = focalLength / Math.max(1, focalLength - rotatedZ * 0.5)
  const restScale = focalLength / Math.max(1, focalLength - z * 0.5)
  return (rotatedX * rotatedScale - x * restScale + silhouetteOffset) * blend
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
