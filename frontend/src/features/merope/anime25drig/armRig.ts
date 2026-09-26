import type { Anime25DPlaybackAnchors, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import { isSkinTone } from './shoulderContact'

/**
 * A sleeve drawing hung from the shoulder it belongs to. Bind-time evidence only:
 * the joint is found on the drawing's own opaque pixels near the anatomical
 * shoulder, never placed at a fixed fraction of the layer box.
 */
export interface ArmRig {
  /** Image-plane rotation sign that swings this arm away from the body. */
  outward: 1 | -1
  pivotX: number
  pivotY: number
  /** Arm radius at the joint; the rotation fades in across it. */
  radius: number
  /** Shoulder to hand, including the part the crop hides; drives inertia. */
  reach: number
  /** Joint to the bottom of the drawing. */
  length: number
  /** A drawing already raised across the body swings less. */
  scale: number
  /** The canvas cut this drawing runs into, if any; a swing slides along it. */
  cutY: number | null
  /**
   * Fabric all the way down, no forearm or hand showing: the lower sleeve is
   * cloth hanging from the arm, so it may trail and sag behind the arm's swing.
   */
  drape: boolean
}

type Layer = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h' | 'side'>
type Anchors = Pick<Anime25DPlaybackAnchors, 'face' | 'neckPivot' | 'neckBottom'>

/** `armY` = 1 raises both shoulders this far, in face-scaled pixels. */
export const ARM_SHRUG = 8

/** Share of the shoulder-to-cut length that bends to keep a cut on the frame. */
export const ARM_CUT_BAND = 0.45

const OPAQUE = 128
/**
 * Any more visible skin in the lower sleeve and it is an arm, not a drape.
 * Fabric, printed flowers included, stays under 4%; a bare forearm is over 25%.
 */
const DRAPE_SKIN = 0.05
const POSED_AREA = 0.05
const POSED_SCALE = 0.3

export function bindArmRig(
  arm: Layer,
  image: CroppedLayerPixels | null,
  anchors: Anchors,
  contentBottom: number,
): ArmRig | null {
  if (!image || (arm.side !== 'L' && arm.side !== 'R')) return null
  const { width, height, pixels } = image
  if (width < 2 || height < 2 || pixels.length !== width * height * 4) return null
  if (![arm.w, arm.h].every((v) => Number.isFinite(v) && v > 0)) return null
  const faceWidth = anchors.face.x1 - anchors.face.x0
  const faceHeight = anchors.face.y1 - anchors.face.y0
  if (!(faceWidth > 0 && faceHeight > 0)) return null
  // Layer 'L' is the image-left sleeve: its shoulder is left of the neck.
  const away = arm.side === 'L' ? -1 : 1
  const shoulderX = anchors.neckPivot.x + away * faceWidth * 0.72
  const shoulderY = anchors.neckBottom + faceHeight * 0.04
  const scaleX = arm.w / width
  const scaleY = arm.h / height
  const opaque = (x: number, y: number) => pixels[(y * width + x) * 4 + 3] >= OPAQUE
  let nearest = Infinity
  let hitX = -1
  let hitY = -1
  let area = 0
  // A shoulder is never above the neck; a hand raised beside the face is not one.
  const shoulderTop = anchors.neckBottom - faceHeight * 0.05
  for (let y = 0; y < height; y++) {
    const above = arm.y + (y + 0.5) * scaleY < shoulderTop
    for (let x = 0; x < width; x++) {
      if (!opaque(x, y)) continue
      area++
      if (above) continue
      const d = Math.hypot(arm.x + (x + 0.5) * scaleX - shoulderX, arm.y + (y + 0.5) * scaleY - shoulderY)
      if (d < nearest) {
        nearest = d
        hitX = x
        hitY = y
      }
    }
  }
  if (area < 16 || hitX < 0) return null
  // The opaque run nearest the contact point on a row: the sleeve, not a
  // detached ribbon or the other side of a gap.
  const run = (y: number): [number, number] | null => {
    let seed = -1
    for (let d = 0; d < width && seed < 0; d++) {
      if (hitX - d >= 0 && opaque(hitX - d, y)) seed = hitX - d
      else if (hitX + d < width && opaque(hitX + d, y)) seed = hitX + d
    }
    if (seed < 0) return null
    let left = seed
    let right = seed
    while (left > 0 && opaque(left - 1, y)) left--
    while (right < width - 1 && opaque(right + 1, y)) right++
    return [left, right]
  }
  // A sleeve top is rounded; measure its girth a little below the contact.
  const girthRow = Math.min(height - 1, hitY + Math.max(1, Math.round(height * 0.06)))
  const girth = run(girthRow) ?? [hitX, hitX]
  const radius = Math.max(2, ((girth[1] - girth[0] + 1) * scaleX) / 2)
  // The joint sits inside the sleeve, below its top contour.
  const jointRow = Math.min(height - 1, hitY + Math.round((radius * 0.7) / scaleY))
  const span = run(jointRow) ?? girth
  const pivotX = arm.x + ((span[0] + span[1] + 1) / 2) * scaleX
  const pivotY = arm.y + (jointRow + 0.5) * scaleY
  // A hand raised above its own shoulder must not be swung like a hanging arm.
  let raised = 0
  const raisedRow = Math.floor((pivotY - radius - arm.y) / scaleY)
  for (let y = 0; y < Math.min(height, raisedRow); y++) {
    for (let x = 0; x < width; x++) {
      if (opaque(x, y)) raised++
    }
  }
  const posed = raised / area > POSED_AREA
  // The portrait crop, not a natural cuff: opaque across the bottom rows. The
  // very last row is often resampled to partial alpha, so read just above it.
  let cutColumns = 0
  for (let x = 0; x < width; x++) {
    if (opaque(x, Math.max(0, height - 2)) && opaque(x, Math.max(0, height - 4))) cutColumns++
  }
  const cut = Math.abs(arm.y + arm.h - contentBottom) <= 0.5 && cutColumns * scaleX >= radius * 0.5
  let lower = 0
  let skin = 0
  for (let y = Math.max(0, Math.floor((pivotY + (arm.y + arm.h - pivotY) * 0.5 - arm.y) / scaleY)); y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (!opaque(x, y)) continue
      lower++
      const i = (y * width + x) * 4
      if (isSkinTone(pixels[i], pixels[i + 1], pixels[i + 2])) skin++
    }
  }
  return {
    outward: arm.side === 'L' ? 1 : -1,
    pivotX,
    pivotY,
    radius,
    reach: Math.max(arm.y + arm.h - pivotY, faceHeight * 2.2),
    length: arm.y + arm.h - pivotY,
    scale: posed ? POSED_SCALE : 1,
    cutY: cut ? arm.y + arm.h : null,
    // A raised arm's lower sleeve is folded around an elbow, not hanging.
    drape: !posed && lower > 0 && skin / lower < DRAPE_SKIN,
  }
}

/** Where along a drape the cloth starts to follow its own swing. */
const DRAPE_START = 0.35

/** 0 on the arm, 1 at the hem: how much of the drape's own swing a point takes. */
export function armDrapeWeight(rig: Readonly<ArmRig>, y: number): number {
  if (!rig.drape) return 0
  const t = Math.max(0, Math.min(1, (y - rig.pivotY - rig.length * DRAPE_START) / (rig.length * (1 - DRAPE_START))))
  return t * t * (3 - 2 * t)
}

export interface ArmRigMesh {
  /** 0 at the joint, 1 once clear of the shoulder: the arm bends there, never tears. */
  weights: Float32Array
  /** The vertex nearest the joint; its motion is the shoulder's. */
  jointVertex: number
}

export function bindArmRigMesh(rig: ArmRig, rest: Float32Array): ArmRigMesh {
  const weights = new Float32Array(rest.length / 2)
  const inner = rig.radius * 0.25
  const outer = rig.radius * 1.5
  let jointVertex = 0
  let nearest = Infinity
  for (let i = 0; i < weights.length; i++) {
    const d = Math.hypot(rest[i * 2] - rig.pivotX, rest[i * 2 + 1] - rig.pivotY)
    const t = Math.max(0, Math.min(1, (d - inner) / (outer - inner)))
    weights[i] = t * t * (3 - 2 * t)
    if (d < nearest) {
      nearest = d
      jointVertex = i
    }
  }
  return { weights, jointVertex }
}

/** Contact shorter than this is two arms brushing, not hands holding each other. */
const MIN_ARM_CONTACT = 6

/**
 * True when the two sleeve drawings touch: hands clasped, one gripping the
 * other. Such arms cannot swing apart without tearing their contact, so they
 * move as one piece with the torso.
 */
export function anime25DArmsTouch(
  first: Layer,
  firstImage: CroppedLayerPixels | null,
  second: Layer,
  secondImage: CroppedLayerPixels | null,
  reach = 2,
): boolean {
  if (!firstImage || !secondImage) return false
  const cell = Math.max(1, reach)
  const key = (x: number, y: number) => `${Math.floor(x / cell)},${Math.floor(y / cell)}`
  const occupied = new Set<string>()
  eachOpaque(second, secondImage, (x, y) => occupied.add(key(x, y)))
  let contact = 0
  eachOpaque(first, firstImage, (x, y) => {
    if (contact >= MIN_ARM_CONTACT) return
    const cx = Math.floor(x / cell)
    const cy = Math.floor(y / cell)
    for (let dy = -1; dy <= 1; dy++) {
      for (let dx = -1; dx <= 1; dx++) {
        if (occupied.has(`${cx + dx},${cy + dy}`)) {
          contact++
          return
        }
      }
    }
  })
  return contact >= MIN_ARM_CONTACT
}

function eachOpaque(layer: Layer, image: CroppedLayerPixels, visit: (x: number, y: number) => void): void {
  const { width, height, pixels } = image
  if (width < 1 || height < 1 || pixels.length !== width * height * 4) return
  const scaleX = layer.w / width
  const scaleY = layer.h / height
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (pixels[(y * width + x) * 4 + 3] < OPAQUE) continue
      visit(layer.x + (x + 0.5) * scaleX, layer.y + (y + 0.5) * scaleY)
    }
  }
}

/** Share of the face's area a hand must cover near it to be resting on the head. */
const HEAD_CONTACT_SHARE = 0.02

/**
 * True when a hand rests on the head: at a cheek, an ear or in the hair. The
 * hand is carried by the body, so a head turning under it would slide out
 * from beneath the fingers or through them.
 */
export function anime25DHandTouchesHead(
  arms: readonly { layer: Layer; image: CroppedLayerPixels | null }[],
  face: Anchors['face'],
): boolean {
  const radiusX = ((face.x1 - face.x0) / 2) * 1.25
  const radiusY = ((face.y1 - face.y0) / 2) * 1.2
  if (!(radiusX > 0 && radiusY > 0)) return false
  const centerX = (face.x0 + face.x1) / 2
  const centerY = (face.y0 + face.y1) / 2
  let covered = 0
  for (const { layer, image } of arms) {
    if (!image) continue
    const pixelArea = (layer.w / image.width) * (layer.h / image.height)
    eachOpaque(layer, image, (x, y) => {
      const dx = (x - centerX) / radiusX
      const dy = (y - centerY) / radiusY
      if (dx * dx + dy * dy <= 1) covered += pixelArea
    })
  }
  return covered >= (face.x1 - face.x0) * (face.y1 - face.y0) * HEAD_CONTACT_SHARE
}
